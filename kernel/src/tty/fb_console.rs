//! High-Performance Framebuffer Text Console Renderer
//!
//! Features:
//! - Full ANSI / VT100 escape sequence decoding (colors, cursor positioning, clear screen/line)
//! - Optimized scanline-unrolled glyph rasterization for 32bpp and standard truecolor
//! - Hardware-accelerated memory moves for instantaneous scrolling and line erasing
//! - Modular integration with Limine GOP linear framebuffer and devfs

use core::ptr;

use super::font::{get_glyph, FONT_HEIGHT, FONT_WIDTH};
use crate::drivers::gpu::framebuffer::{FramebufferInfo, FRAMEBUFFER};
use crate::sync::spinlock::Spinlock;

/// Foreground/background RGB color pair for a text cell.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CellColor {
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
}

impl CellColor {
    pub const DEFAULT: Self = Self {
        fg: (200, 200, 200),
        bg: (15, 15, 20),
    };
}

/// Convert (R, G, B) into a packed framebuffer pixel color.
#[inline(always)]
fn encode_pixel(info: &FramebufferInfo, r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << info.red_mask_shift)
        | ((g as u32) << info.green_mask_shift)
        | ((b as u32) << info.blue_mask_shift)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AnsiParseState {
    Normal,
    Escape,
    Csi,
}

/// Framebuffer text console renderer.
pub struct FbConsole {
    cursor_x: usize,
    cursor_y: usize,
    cols: usize,
    rows: usize,
    color: CellColor,
    saved_x: usize,
    saved_y: usize,
    ansi_state: AnsiParseState,
    params: [u32; 8],
    param_count: usize,
    is_private: bool,
    bold: bool,
}

impl FbConsole {
    pub const fn new(cols: usize, rows: usize) -> Self {
        Self {
            cursor_x: 0,
            cursor_y: 0,
            cols,
            rows,
            color: CellColor::DEFAULT,
            saved_x: 0,
            saved_y: 0,
            ansi_state: AnsiParseState::Normal,
            params: [0; 8],
            param_count: 0,
            is_private: false,
            bold: false,
        }
    }

    /// Fast clear of the entire screen.
    pub fn clear(&mut self) {
        let guard = FRAMEBUFFER.lock();
        if let Some(ref fb) = *guard {
            let info = fb.info();
            let bg_pixel = encode_pixel(info, self.color.bg.0, self.color.bg.1, self.color.bg.2);
            let bpp = info.bpp / 8;

            if bpp == 4 {
                let total_pixels = info.height * (info.pitch / 4);
                // SAFETY: Framebuffer address is mapped and valid for total_pixels * 4 bytes.
                unsafe {
                    let ptr = info.addr as *mut u32;
                    for i in 0..total_pixels {
                        *ptr.add(i) = bg_pixel;
                    }
                }
            } else {
                // SAFETY: Framebuffer address is mapped.
                unsafe {
                    ptr::write_bytes(info.addr, 0, fb.len());
                }
            }
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    /// Fast scanline-unrolled glyph rasterization (8x16 pixels).
    #[inline]
    fn draw_glyph(info: &FramebufferInfo, col: usize, row: usize, c: char, color: &CellColor) {
        let px = col * FONT_WIDTH;
        let py = row * FONT_HEIGHT;
        if px + FONT_WIDTH > info.width || py + FONT_HEIGHT > info.height {
            return;
        }

        let glyph = get_glyph(c);
        let fg = encode_pixel(info, color.fg.0, color.fg.1, color.fg.2);
        let bg = encode_pixel(info, color.bg.0, color.bg.1, color.bg.2);
        let bpp = info.bpp / 8;

        if bpp == 4 {
            let pitch_words = info.pitch / 4;
            // SAFETY: Bounds verified above, pointer is valid for 16 lines of 8 u32 pixels.
            unsafe {
                let base = (info.addr as *mut u32).add(py * pitch_words + px);
                for (row_i, &byte) in glyph.iter().enumerate() {
                    let line = base.add(row_i * pitch_words);
                    *line.add(0) = if (byte & 0x80) != 0 { fg } else { bg };
                    *line.add(1) = if (byte & 0x40) != 0 { fg } else { bg };
                    *line.add(2) = if (byte & 0x20) != 0 { fg } else { bg };
                    *line.add(3) = if (byte & 0x10) != 0 { fg } else { bg };
                    *line.add(4) = if (byte & 0x08) != 0 { fg } else { bg };
                    *line.add(5) = if (byte & 0x04) != 0 { fg } else { bg };
                    *line.add(6) = if (byte & 0x02) != 0 { fg } else { bg };
                    *line.add(7) = if (byte & 0x01) != 0 { fg } else { bg };
                }
            }
        }
    }

    /// Fast single-cell erase.
    #[inline]
    fn erase_cell(info: &FramebufferInfo, col: usize, row: usize, color: &CellColor) {
        let px = col * FONT_WIDTH;
        let py = row * FONT_HEIGHT;
        if px + FONT_WIDTH > info.width || py + FONT_HEIGHT > info.height {
            return;
        }
        let bg = encode_pixel(info, color.bg.0, color.bg.1, color.bg.2);
        let pitch_words = info.pitch / 4;
        // SAFETY: Within framebuffer boundaries.
        unsafe {
            let base = (info.addr as *mut u32).add(py * pitch_words + px);
            for row_i in 0..FONT_HEIGHT {
                let line = base.add(row_i * pitch_words);
                for col_i in 0..FONT_WIDTH {
                    *line.add(col_i) = bg;
                }
            }
        }
    }

    /// Fast text row erase.
    fn erase_row(info: &FramebufferInfo, row: usize, cols: usize, color: &CellColor) {
        let py = row * FONT_HEIGHT;
        let px_max = core::cmp::min(cols * FONT_WIDTH, info.width);
        if py + FONT_HEIGHT > info.height {
            return;
        }
        let bg = encode_pixel(info, color.bg.0, color.bg.1, color.bg.2);
        let pitch_words = info.pitch / 4;
        // SAFETY: Memory is within mapped framebuffer boundaries.
        unsafe {
            for row_i in 0..FONT_HEIGHT {
                let line = (info.addr as *mut u32).add((py + row_i) * pitch_words);
                for x in 0..px_max {
                    *line.add(x) = bg;
                }
            }
        }
    }

    /// Fast scroll up by 1 line using SIMD/vector memory copy.
    fn scroll_up(info: &FramebufferInfo, rows: usize, cols: usize, color: &CellColor) {
        let row_bytes = FONT_HEIGHT * info.pitch;
        let copy_bytes = (rows.saturating_sub(1)) * row_bytes;
        // SAFETY: Source and destination memory regions reside in mapped framebuffer.
        unsafe {
            ptr::copy(info.addr.add(row_bytes), info.addr, copy_bytes);
        }
        Self::erase_row(info, rows.saturating_sub(1), cols, color);
    }

    /// Advance to the next line, scrolling if the cursor reaches the bottom row.
    fn newline(&mut self) {
        self.cursor_x = 0;
        self.cursor_y += 1;
        if self.cursor_y >= self.rows {
            let guard = FRAMEBUFFER.lock();
            if let Some(ref fb) = *guard {
                Self::scroll_up(fb.info(), self.rows, self.cols, &self.color);
            }
            self.cursor_y = self.rows.saturating_sub(1);
        }
    }

    /// Erase from cursor position to end of line.
    fn erase_line_from_cursor(&mut self) {
        let guard = FRAMEBUFFER.lock();
        if let Some(ref fb) = *guard {
            let info = fb.info();
            for col in self.cursor_x..self.cols {
                Self::erase_cell(info, col, self.cursor_y, &self.color);
            }
        }
    }

    /// Erase from start of line to current cursor position.
    fn erase_line_to_cursor(&mut self) {
        let guard = FRAMEBUFFER.lock();
        if let Some(ref fb) = *guard {
            let info = fb.info();
            let limit = core::cmp::min(self.cursor_x + 1, self.cols);
            for col in 0..limit {
                Self::erase_cell(info, col, self.cursor_y, &self.color);
            }
        }
    }

    /// Erase the entire current line.
    fn erase_current_line(&mut self) {
        let guard = FRAMEBUFFER.lock();
        if let Some(ref fb) = *guard {
            Self::erase_row(fb.info(), self.cursor_y, self.cols, &self.color);
        }
    }

    /// Process SGR color codes into RGB.
    fn sgr_to_rgb(idx: u32, bold: bool) -> (u8, u8, u8) {
        match (idx, bold) {
            (30, false) | (30, true) => (0, 0, 0),
            (31, false) => (178, 34, 34),   (31, true) => (255, 85, 85),
            (32, false) => (46, 139, 87),   (32, true) => (85, 255, 85),
            (33, false) => (180, 100, 0),   (33, true) => (255, 255, 85),
            (34, false) => (65, 105, 225),  (34, true) => (100, 149, 237),
            (35, false) => (148, 0, 211),   (35, true) => (218, 112, 214),
            (36, false) => (0, 139, 139),   (36, true) => (0, 255, 255),
            (37, false) => (200, 200, 200), (37, true) => (255, 255, 255),
            (90, _) => (105, 105, 105), (91, _) => (255, 85, 85),
            (92, _) => (85, 255, 85),   (93, _) => (255, 255, 85),
            (94, _) => (100, 149, 237), (95, _) => (218, 112, 214),
            (96, _) => (0, 255, 255),   (97, _) => (255, 255, 255),
            (40, _) => (0, 0, 0),       (41, _) => (178, 34, 34),
            (42, _) => (46, 139, 87),   (43, _) => (180, 100, 0),
            (44, _) => (65, 105, 225),  (45, _) => (148, 0, 211),
            (46, _) => (0, 139, 139),   (47, _) => (200, 200, 200),
            (100, _) => (105, 105, 105), (101, _) => (255, 85, 85),
            (102, _) => (85, 255, 85),   (103, _) => (255, 255, 85),
            (104, _) => (100, 149, 237), (105, _) => (218, 112, 214),
            (106, _) => (0, 255, 255),   (107, _) => (255, 255, 255),
            _ => (200, 200, 200),
        }
    }

    /// Process a CSI ANSI command sequence.
    fn process_csi(&mut self, cmd: u8) {
        let count = if self.param_count == 0 { 0 } else { self.param_count };
        let params = &self.params[..count];

        match cmd {
            b'm' => {
                if params.is_empty() || (params.len() == 1 && params[0] == 0) {
                    self.color = CellColor::DEFAULT;
                    self.bold = false;
                    return;
                }
                for &p in params {
                    match p {
                        0 => { self.color = CellColor::DEFAULT; self.bold = false; }
                        1 => { self.bold = true; }
                        22 => { self.bold = false; }
                        30..=37 | 90..=97 => self.color.fg = Self::sgr_to_rgb(p, self.bold),
                        39 => self.color.fg = CellColor::DEFAULT.fg,
                        40..=47 | 100..=107 => self.color.bg = Self::sgr_to_rgb(p, false),
                        49 => self.color.bg = CellColor::DEFAULT.bg,
                        _ => {}
                    }
                }
            }
            b'H' | b'f' => {
                let row = params.get(0).copied().unwrap_or(1).saturating_sub(1) as usize;
                let col = params.get(1).copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor_y = core::cmp::min(row, self.rows.saturating_sub(1));
                self.cursor_x = core::cmp::min(col, self.cols.saturating_sub(1));
            }
            b'A' => {
                let n = params.get(0).copied().unwrap_or(1).max(1) as usize;
                self.cursor_y = self.cursor_y.saturating_sub(n);
            }
            b'B' => {
                let n = params.get(0).copied().unwrap_or(1).max(1) as usize;
                self.cursor_y = core::cmp::min(self.cursor_y + n, self.rows.saturating_sub(1));
            }
            b'C' => {
                let n = params.get(0).copied().unwrap_or(1).max(1) as usize;
                self.cursor_x = core::cmp::min(self.cursor_x + n, self.cols.saturating_sub(1));
            }
            b'D' => {
                let n = params.get(0).copied().unwrap_or(1).max(1) as usize;
                self.cursor_x = self.cursor_x.saturating_sub(n);
            }
            b'G' | b'`' => {
                let col = params.get(0).copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor_x = core::cmp::min(col, self.cols.saturating_sub(1));
            }
            b'd' => {
                let row = params.get(0).copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor_y = core::cmp::min(row, self.rows.saturating_sub(1));
            }
            b'J' => match params.get(0).copied().unwrap_or(0) {
                0 => {
                    self.erase_line_from_cursor();
                    let cur = self.cursor_y;
                    let guard = FRAMEBUFFER.lock();
                    if let Some(ref fb) = *guard {
                        for row in (cur + 1)..self.rows {
                            Self::erase_row(fb.info(), row, self.cols, &self.color);
                        }
                    }
                }
                1 => {
                    let cur = self.cursor_y;
                    let guard = FRAMEBUFFER.lock();
                    if let Some(ref fb) = *guard {
                        for row in 0..cur {
                            Self::erase_row(fb.info(), row, self.cols, &self.color);
                        }
                    }
                    self.erase_line_to_cursor();
                }
                2 | 3 => self.clear(),
                _ => {}
            },
            b'K' => match params.get(0).copied().unwrap_or(0) {
                0 => self.erase_line_from_cursor(),
                1 => self.erase_line_to_cursor(),
                2 => self.erase_current_line(),
                _ => {}
            },
            b's' => { self.saved_x = self.cursor_x; self.saved_y = self.cursor_y; }
            b'u' => {
                self.cursor_x = core::cmp::min(self.saved_x, self.cols.saturating_sub(1));
                self.cursor_y = core::cmp::min(self.saved_y, self.rows.saturating_sub(1));
            }
            _ => {}
        }
    }

    /// Process a single byte through the terminal emulator and render onto the framebuffer.
    pub fn write_byte(&mut self, byte: u8) {
        match self.ansi_state {
            AnsiParseState::Normal => {
                if byte == 0x1B {
                    self.ansi_state = AnsiParseState::Escape;
                    self.params = [0; 8];
                    self.param_count = 0;
                    self.is_private = false;
                } else {
                    self.handle_char(byte);
                }
            }
            AnsiParseState::Escape => match byte {
                b'[' => { self.ansi_state = AnsiParseState::Csi; }
                b'7' => {
                    self.saved_x = self.cursor_x;
                    self.saved_y = self.cursor_y;
                    self.ansi_state = AnsiParseState::Normal;
                }
                b'8' => {
                    self.cursor_x = core::cmp::min(self.saved_x, self.cols.saturating_sub(1));
                    self.cursor_y = core::cmp::min(self.saved_y, self.rows.saturating_sub(1));
                    self.ansi_state = AnsiParseState::Normal;
                }
                b'c' => {
                    self.color = CellColor::DEFAULT;
                    self.clear();
                    self.ansi_state = AnsiParseState::Normal;
                }
                _ => { self.ansi_state = AnsiParseState::Normal; }
            },
            AnsiParseState::Csi => match byte {
                b'?' => { self.is_private = true; }
                b'0'..=b'9' => {
                    if self.param_count == 0 { self.param_count = 1; }
                    let idx = self.param_count - 1;
                    if idx < self.params.len() {
                        self.params[idx] = self.params[idx].saturating_mul(10).saturating_add((byte - b'0') as u32);
                    }
                }
                b';' => {
                    if self.param_count < self.params.len() { self.param_count += 1; }
                }
                cmd => {
                    self.process_csi(cmd);
                    self.ansi_state = AnsiParseState::Normal;
                }
            },
        }
    }

    /// Process printable or standard ASCII control characters.
    #[inline]
    fn handle_char(&mut self, byte: u8) {
        match byte {
            b'\n' => { self.newline(); }
            b'\r' => { self.cursor_x = 0; }
            b'\t' => {
                let next_tab = (self.cursor_x + 8) & !7;
                if next_tab >= self.cols {
                    self.newline();
                } else {
                    self.cursor_x = next_tab;
                }
            }
            0x08 | 0x7F => {
                // Backspace
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                    let guard = FRAMEBUFFER.lock();
                    if let Some(ref fb) = *guard {
                        Self::erase_cell(fb.info(), self.cursor_x, self.cursor_y, &self.color);
                    }
                }
            }
            b if (0x20..=0x7E).contains(&b) => {
                let guard = FRAMEBUFFER.lock();
                if let Some(ref fb) = *guard {
                    Self::draw_glyph(fb.info(), self.cursor_x, self.cursor_y, b as char, &self.color);
                }
                drop(guard);

                self.cursor_x += 1;
                if self.cursor_x >= self.cols {
                    self.newline();
                }
            }
            _ => {}
        }
    }

    /// Write a string slice.
    pub fn write_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.write_byte(b);
        }
    }
}

/// Query active console text dimensions (rows, cols) from Framebuffer geometry.
pub fn fb_get_console_size() -> Option<(usize, usize)> {
    let guard = FRAMEBUFFER.lock();
    if let Some(ref fb) = *guard {
        let info = fb.info();
        let cols = info.width / FONT_WIDTH;
        let rows = info.height / FONT_HEIGHT;
        Some((rows, cols))
    } else {
        None
    }
}

/// Global framebuffer text console terminal instance.
pub static FB_TERMINAL: Spinlock<FbConsole> = Spinlock::new(FbConsole::new(80, 25));

/// Initialize the framebuffer text console.
pub fn fb_console_init() {
    if let Some((rows, cols)) = fb_get_console_size() {
        let mut t = FB_TERMINAL.lock();
        t.cols = cols;
        t.rows = rows;
        t.clear();
        log::info!("[TTY/FB] Framebuffer text console ready: {}x{} chars.", cols, rows);
    }
}

/// Write a single byte to the framebuffer console.
#[inline(always)]
pub fn fb_console_write_byte(byte: u8) {
    FB_TERMINAL.lock().write_byte(byte);
}

/// Write a string to the framebuffer console.
pub fn fb_console_write_str(s: &str) {
    FB_TERMINAL.lock().write_str(s);
}

/// Check if the framebuffer text console is currently available.
#[inline(always)]
pub fn fb_console_available() -> bool {
    FRAMEBUFFER.lock().is_some()
}
