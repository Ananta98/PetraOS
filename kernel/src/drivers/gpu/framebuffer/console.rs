//! Framebuffer Text Console (fbcon)
//!
//! Provides a text console over the linear framebuffer with ANSI escape
//! sequence parsing, scrolling, color management, and cursor control.

use alloc::vec::Vec;
use super::fb::{Color, FRAMEBUFFER};
use super::font::{FONT_HEIGHT, FONT_WIDTH};
use crate::sync::spinlock::Spinlock;

/// Global framebuffer text console instance.
pub static FB_CONSOLE: Spinlock<Option<FbConsole>> = Spinlock::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnsiState {
    Normal,
    Esc,
    Bracket,
    Osc,
    Charset,
}

/// Text console rendering onto linear framebuffer.
pub struct FbConsole {
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub cols: usize,
    pub rows: usize,
    pub fg: Color,
    pub bg: Color,
    pub default_fg: Color,
    pub default_bg: Color,
    pub cursor_visible: bool,
    pub saved_cursor_x: usize,
    pub saved_cursor_y: usize,
    text_buffer: Vec<(char, Color, Color)>,
    ansi_state: AnsiState,
    ansi_params: [u32; 8],
    ansi_param_count: usize,
    ansi_current_param: u32,
    ansi_has_param: bool,
    ansi_is_private: bool,
}

impl FbConsole {
    /// Creates a new FbConsole for the given framebuffer dimensions.
    pub fn new(fb_width: usize, fb_height: usize) -> Self {
        let cols = fb_width / FONT_WIDTH;
        let rows = fb_height / FONT_HEIGHT;
        let c = if cols > 0 { cols } else { 80 };
        let r = if rows > 0 { rows } else { 25 };

        let mut text_buffer = Vec::with_capacity(c * r);
        text_buffer.resize(c * r, (' ', Color::WHITE, Color::rgb(15, 15, 20)));

        Self {
            cursor_x: 0,
            cursor_y: 0,
            cols: c,
            rows: r,
            fg: Color::WHITE,
            bg: Color::rgb(15, 15, 20),
            default_fg: Color::WHITE,
            default_bg: Color::rgb(15, 15, 20),
            cursor_visible: true,
            saved_cursor_x: 0,
            saved_cursor_y: 0,
            text_buffer,
            ansi_state: AnsiState::Normal,
            ansi_params: [0; 8],
            ansi_param_count: 0,
            ansi_current_param: 0,
            ansi_has_param: false,
            ansi_is_private: false,
        }
    }

    #[inline(always)]
    fn get_cell(&self, x: usize, y: usize) -> (char, Color, Color) {
        if x < self.cols && y < self.rows {
            self.text_buffer[y * self.cols + x]
        } else {
            (' ', self.default_fg, self.default_bg)
        }
    }

    #[inline(always)]
    fn set_cell(&mut self, x: usize, y: usize, ch: char, fg: Color, bg: Color) {
        if x < self.cols && y < self.rows {
            self.text_buffer[y * self.cols + x] = (ch, fg, bg);
        }
    }

    /// Draws the cursor at the current (cursor_x, cursor_y) position.
    pub fn draw_cursor(&mut self) {
        if !self.cursor_visible || self.cursor_x >= self.cols || self.cursor_y >= self.rows {
            return;
        }
        if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
            let px = self.cursor_x * FONT_WIDTH;
            let py = self.cursor_y * FONT_HEIGHT;
            if px < fb.width() && py < fb.height() {
                let cursor_h = 2;
                let cy = py + FONT_HEIGHT.saturating_sub(cursor_h);
                fb.fill_rect(px, cy, FONT_WIDTH, cursor_h, self.fg);
            }
        }
    }

    /// Erases the cursor at the current (cursor_x, cursor_y) position.
    pub fn erase_cursor(&mut self) {
        if self.cursor_x >= self.cols || self.cursor_y >= self.rows {
            return;
        }
        let (ch, fg, bg) = self.get_cell(self.cursor_x, self.cursor_y);
        if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
            let px = self.cursor_x * FONT_WIDTH;
            let py = self.cursor_y * FONT_HEIGHT;
            if px < fb.width() && py < fb.height() {
                if ch != ' ' && ch != '\0' {
                    fb.draw_char(px, py, ch, fg, bg);
                } else {
                    fb.fill_rect(px, py, FONT_WIDTH, FONT_HEIGHT, bg);
                }
            }
        }
    }

    /// Clears the text console and resets cursor position to top-left.
    pub fn clear(&mut self) {
        self.erase_cursor();
        if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
            fb.clear(self.bg);
        }
        for cell in self.text_buffer.iter_mut() {
            *cell = (' ', self.default_fg, self.default_bg);
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.draw_cursor();
    }

    /// Scrolls the console buffer upwards by one row.
    pub fn scroll(&mut self) {
        self.erase_cursor();
        if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
            fb.scroll_up(FONT_HEIGHT, self.bg);
        }
        if self.rows > 1 {
            for y in 0..(self.rows - 1) {
                let dst_start = y * self.cols;
                let src_start = (y + 1) * self.cols;
                for x in 0..self.cols {
                    self.text_buffer[dst_start + x] = self.text_buffer[src_start + x];
                }
            }
            let last_row_start = (self.rows - 1) * self.cols;
            for x in 0..self.cols {
                self.text_buffer[last_row_start + x] = (' ', self.default_fg, self.default_bg);
            }
        }
    }

    /// Outputs a single byte to the console.
    pub fn write_byte(&mut self, byte: u8) {
        match self.ansi_state {
            AnsiState::Normal => {
                if byte == 0x1B {
                    self.ansi_state = AnsiState::Esc;
                } else {
                    self.handle_char(byte as char);
                }
            }
            AnsiState::Esc => match byte {
                b'[' => {
                    self.ansi_state = AnsiState::Bracket;
                    self.ansi_params = [0; 8];
                    self.ansi_param_count = 0;
                    self.ansi_current_param = 0;
                    self.ansi_has_param = false;
                    self.ansi_is_private = false;
                }
                b']' => {
                    self.ansi_state = AnsiState::Osc;
                }
                b'(' | b')' => {
                    self.ansi_state = AnsiState::Charset;
                }
                0x1B => {
                    // Stay in Esc
                }
                _ => {
                    self.ansi_state = AnsiState::Normal;
                }
            },
            AnsiState::Osc => {
                if byte == 0x07 || byte == 0x1B {
                    self.ansi_state = AnsiState::Normal;
                }
            }
            AnsiState::Charset => {
                self.ansi_state = AnsiState::Normal;
            }
            AnsiState::Bracket => {
                if byte == b'?' || byte == b'>' {
                    self.ansi_is_private = true;
                } else if byte.is_ascii_digit() {
                    self.ansi_current_param = self
                        .ansi_current_param
                        .saturating_mul(10)
                        .saturating_add((byte - b'0') as u32);
                    self.ansi_has_param = true;
                } else if byte == b';' {
                    if self.ansi_param_count < self.ansi_params.len() {
                        self.ansi_params[self.ansi_param_count] = self.ansi_current_param;
                        self.ansi_param_count += 1;
                    }
                    self.ansi_current_param = 0;
                    self.ansi_has_param = false;
                } else {
                    // Final command character
                    if self.ansi_has_param && self.ansi_param_count < self.ansi_params.len() {
                        self.ansi_params[self.ansi_param_count] = self.ansi_current_param;
                        self.ansi_param_count += 1;
                    }
                    self.apply_ansi_command(byte);
                    self.ansi_state = AnsiState::Normal;
                    self.ansi_is_private = false;
                }
            }
        }
    }

    /// Process ANSI escape command code.
    fn apply_ansi_command(&mut self, cmd: u8) {
        self.erase_cursor();
        match cmd {
            b'm' => {
                // Select Graphic Rendition (SGR)
                if self.ansi_param_count == 0 {
                    self.fg = self.default_fg;
                    self.bg = self.default_bg;
                    self.draw_cursor();
                    return;
                }

                for i in 0..self.ansi_param_count {
                    match self.ansi_params[i] {
                        0 => {
                            self.fg = self.default_fg;
                            self.bg = self.default_bg;
                        }
                        1 => {
                            // Bold / bright foreground
                            self.fg = Color::BRIGHT_WHITE;
                        }
                        30 => self.fg = Color::BLACK,
                        31 => self.fg = Color::RED,
                        32 => self.fg = Color::GREEN,
                        33 => self.fg = Color::YELLOW,
                        34 => self.fg = Color::BLUE,
                        35 => self.fg = Color::MAGENTA,
                        36 => self.fg = Color::CYAN,
                        37 => self.fg = Color::LIGHT_GRAY,
                        39 => self.fg = self.default_fg,
                        40 => self.bg = Color::BLACK,
                        41 => self.bg = Color::RED,
                        42 => self.bg = Color::GREEN,
                        43 => self.bg = Color::YELLOW,
                        44 => self.bg = Color::BLUE,
                        45 => self.bg = Color::MAGENTA,
                        46 => self.bg = Color::CYAN,
                        47 => self.bg = Color::LIGHT_GRAY,
                        49 => self.bg = self.default_bg,
                        90 => self.fg = Color::BRIGHT_BLACK,
                        91 => self.fg = Color::BRIGHT_RED,
                        92 => self.fg = Color::BRIGHT_GREEN,
                        93 => self.fg = Color::BRIGHT_YELLOW,
                        94 => self.fg = Color::BRIGHT_BLUE,
                        95 => self.fg = Color::BRIGHT_MAGENTA,
                        96 => self.fg = Color::BRIGHT_CYAN,
                        97 => self.fg = Color::BRIGHT_WHITE,
                        _ => {}
                    }
                }
            }
            b'J' => {
                // Erase in display
                let mode = if self.ansi_param_count > 0 {
                    self.ansi_params[0]
                } else {
                    0
                };
                if mode == 2 || mode == 3 {
                    self.clear();
                    return;
                } else if mode == 0 {
                    // Erase from cursor to end of screen
                    if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
                        let px = self.cursor_x * FONT_WIDTH;
                        let py = self.cursor_y * FONT_HEIGHT;
                        if px < fb.width() {
                            fb.fill_rect(px, py, fb.width() - px, FONT_HEIGHT, self.bg);
                        }
                        let rest_y = (self.cursor_y + 1) * FONT_HEIGHT;
                        if rest_y < fb.height() {
                            fb.fill_rect(0, rest_y, fb.width(), fb.height() - rest_y, self.bg);
                        }
                    }
                    for y in self.cursor_y..self.rows {
                        let start_x = if y == self.cursor_y { self.cursor_x } else { 0 };
                        for x in start_x..self.cols {
                            self.set_cell(x, y, ' ', self.default_fg, self.default_bg);
                        }
                    }
                }
            }
            b'K' => {
                // Erase in line
                let mode = if self.ansi_param_count > 0 {
                    self.ansi_params[0]
                } else {
                    0
                };
                let py = self.cursor_y * FONT_HEIGHT;
                if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
                    match mode {
                        0 => {
                            let px = self.cursor_x * FONT_WIDTH;
                            if px < fb.width() {
                                fb.fill_rect(px, py, fb.width() - px, FONT_HEIGHT, self.bg);
                            }
                        }
                        1 => {
                            let px = (self.cursor_x + 1) * FONT_WIDTH;
                            fb.fill_rect(0, py, core::cmp::min(px, fb.width()), FONT_HEIGHT, self.bg);
                        }
                        2 => {
                            fb.fill_rect(0, py, fb.width(), FONT_HEIGHT, self.bg);
                        }
                        _ => {}
                    }
                }
                match mode {
                    0 => {
                        for x in self.cursor_x..self.cols {
                            self.set_cell(x, self.cursor_y, ' ', self.default_fg, self.default_bg);
                        }
                    }
                    1 => {
                        for x in 0..=core::cmp::min(self.cursor_x, self.cols.saturating_sub(1)) {
                            self.set_cell(x, self.cursor_y, ' ', self.default_fg, self.default_bg);
                        }
                    }
                    2 => {
                        for x in 0..self.cols {
                            self.set_cell(x, self.cursor_y, ' ', self.default_fg, self.default_bg);
                        }
                    }
                    _ => {}
                }
            }
            b'H' | b'f' => {
                // Cursor position
                let row = if self.ansi_param_count > 0 && self.ansi_params[0] > 0 {
                    (self.ansi_params[0] - 1) as usize
                } else {
                    0
                };
                let col = if self.ansi_param_count > 1 && self.ansi_params[1] > 0 {
                    (self.ansi_params[1] - 1) as usize
                } else {
                    0
                };
                self.cursor_y = core::cmp::min(row, self.rows.saturating_sub(1));
                self.cursor_x = core::cmp::min(col, self.cols.saturating_sub(1));
            }
            b'A' => {
                // Cursor Up
                let n = if self.ansi_param_count > 0 && self.ansi_params[0] > 0 {
                    self.ansi_params[0] as usize
                } else {
                    1
                };
                self.cursor_y = self.cursor_y.saturating_sub(n);
            }
            b'B' => {
                // Cursor Down
                let n = if self.ansi_param_count > 0 && self.ansi_params[0] > 0 {
                    self.ansi_params[0] as usize
                } else {
                    1
                };
                self.cursor_y = core::cmp::min(self.cursor_y + n, self.rows.saturating_sub(1));
            }
            b'C' => {
                // Cursor Forward
                let n = if self.ansi_param_count > 0 && self.ansi_params[0] > 0 {
                    self.ansi_params[0] as usize
                } else {
                    1
                };
                self.cursor_x = core::cmp::min(self.cursor_x + n, self.cols.saturating_sub(1));
            }
            b'D' => {
                // Cursor Backward
                let n = if self.ansi_param_count > 0 && self.ansi_params[0] > 0 {
                    self.ansi_params[0] as usize
                } else {
                    1
                };
                self.cursor_x = self.cursor_x.saturating_sub(n);
            }
            b'G' | b'`' => {
                // Cursor Horizontal Absolute (column)
                let col = if self.ansi_param_count > 0 && self.ansi_params[0] > 0 {
                    (self.ansi_params[0] - 1) as usize
                } else {
                    0
                };
                self.cursor_x = core::cmp::min(col, self.cols.saturating_sub(1));
            }
            b'd' => {
                // Line Position Absolute (row)
                let row = if self.ansi_param_count > 0 && self.ansi_params[0] > 0 {
                    (self.ansi_params[0] - 1) as usize
                } else {
                    0
                };
                self.cursor_y = core::cmp::min(row, self.rows.saturating_sub(1));
            }
            b's' => {
                // Save cursor position
                self.saved_cursor_x = self.cursor_x;
                self.saved_cursor_y = self.cursor_y;
            }
            b'u' => {
                // Restore cursor position
                self.cursor_x = core::cmp::min(self.saved_cursor_x, self.cols.saturating_sub(1));
                self.cursor_y = core::cmp::min(self.saved_cursor_y, self.rows.saturating_sub(1));
            }
            b'h' => {
                // Set Mode (e.g. \x1B[?25h -> show cursor)
                if self.ansi_is_private && self.ansi_param_count > 0 && self.ansi_params[0] == 25 {
                    self.cursor_visible = true;
                }
            }
            b'l' => {
                // Reset Mode (e.g. \x1B[?25l -> hide cursor)
                if self.ansi_is_private && self.ansi_param_count > 0 && self.ansi_params[0] == 25 {
                    self.cursor_visible = false;
                }
            }
            _ => {}
        }
        self.draw_cursor();
    }

    /// Handles standard printable character and control codes.
    fn handle_char(&mut self, c: char) {
        self.erase_cursor();
        match c {
            '\n' => {
                self.cursor_x = 0;
                self.cursor_y += 1;
                if self.cursor_y >= self.rows {
                    self.scroll();
                    self.cursor_y = self.rows - 1;
                }
            }
            '\r' => {
                self.cursor_x = 0;
            }
            '\t' => {
                let next_tab = (self.cursor_x + 4) & !3;
                if next_tab >= self.cols {
                    self.cursor_x = 0;
                    self.cursor_y += 1;
                    if self.cursor_y >= self.rows {
                        self.scroll();
                        self.cursor_y = self.rows - 1;
                    }
                } else {
                    self.cursor_x = next_tab;
                }
            }
            '\x08' | '\x7F' => {
                // Backspace
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                    self.set_cell(self.cursor_x, self.cursor_y, ' ', self.fg, self.bg);
                    if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
                        fb.fill_rect(
                            self.cursor_x * FONT_WIDTH,
                            self.cursor_y * FONT_HEIGHT,
                            FONT_WIDTH,
                            FONT_HEIGHT,
                            self.bg,
                        );
                    }
                }
            }
            ch => {
                let code = ch as u32;
                if (0x20..=0x7E).contains(&code) {
                    let pixel_x = self.cursor_x * FONT_WIDTH;
                    let pixel_y = self.cursor_y * FONT_HEIGHT;

                    self.set_cell(self.cursor_x, self.cursor_y, ch, self.fg, self.bg);

                    if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
                        fb.draw_char(pixel_x, pixel_y, ch, self.fg, self.bg);
                    }

                    self.cursor_x += 1;
                    if self.cursor_x >= self.cols {
                        self.cursor_x = 0;
                        self.cursor_y += 1;
                        if self.cursor_y >= self.rows {
                            self.scroll();
                            self.cursor_y = self.rows - 1;
                        }
                    }
                }
            }
        }
        self.draw_cursor();
    }

    /// Outputs a string to the console.
    pub fn write_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.write_byte(b);
        }
    }
}

/// Global helper function to write a string to the framebuffer console.
pub fn fb_console_write_str(s: &str) {
    if let Some(ref mut con) = *FB_CONSOLE.lock() {
        con.write_str(s);
    }
}

/// Global helper function to write a single byte to the framebuffer console.
pub fn fb_console_write_byte(b: u8) {
    if let Some(ref mut con) = *FB_CONSOLE.lock() {
        con.write_byte(b);
    }
}

/// Get current framebuffer console rows and columns (dimensions).
pub fn fb_console_get_dimensions() -> Option<(usize, usize)> {
    FB_CONSOLE.lock().as_ref().map(|c| (c.rows, c.cols))
}

