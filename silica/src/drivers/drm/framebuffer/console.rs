// SPDX-License-Identifier: MPL-2.0

//! Framebuffer Console.
//!
//! Maintains an in-memory shadow buffer mirroring the framebuffer content for
//! high-performance, flicker-free rendering and scrolling.

use alloc::{sync::Arc, vec, vec::Vec};
use ostd::mm::HasSize;

use super::{
    ansi::{EraseInDisplay, EraseInLine, EscapeFsm, EscapeOp},
    font::BitmapFont,
    framebuffer::FrameBuffer,
    mode::ConsoleMode,
    pixel::Pixel,
};
use spin::{Mutex, Once};

use crate::drivers::char::tty;

static CONSOLE: Once<Mutex<FramebufferConsole>> = Once::new();

pub fn init(framebuffer: Arc<FrameBuffer>) {
    let console = FramebufferConsole::new(framebuffer);
    CONSOLE.call_once(|| Mutex::new(console));
    tty::update_console_winsize();
}

pub fn get_console() -> Option<&'static Mutex<FramebufferConsole>> {
    CONSOLE.get()
}

/// A text console rendered onto the linear framebuffer.
pub struct FramebufferConsole {
    state: ConsoleState,
    escape_fsm: EscapeFsm,
}

pub type FbConsole = FramebufferConsole;
pub use super::ansi::CursorStyle;

#[derive(Clone, Copy, Debug)]
pub enum ConsoleSetFontError {
    InvalidFont,
}

impl FramebufferConsole {
    /// Creates a new framebuffer console, activated by default.
    pub fn new(framebuffer: Arc<FrameBuffer>) -> Self {
        let mut state = ConsoleState::new(framebuffer);
        state.is_active = true;
        let escape_fsm = EscapeFsm::new();

        let mut cons = Self { state, escape_fsm };
        cons.state.flush_fullscreen();
        cons.state.draw_cursor();
        cons
    }

    /// Activates the console and flushes shadow buffer to hardware display.
    pub fn activate(&mut self) {
        let old_active = self.state.is_active;
        self.state.is_active = true;
        if !old_active {
            self.state.flush_fullscreen();
            self.state.draw_cursor();
        }
    }

    /// Deactivates hardware rendering.
    pub fn deactivate(&mut self) {
        self.state.erase_cursor();
        self.state.is_active = false;
    }

    /// Returns the current console mode.
    pub fn mode(&self) -> ConsoleMode {
        self.state.mode
    }

    /// Sets the console mode (Text or Graphics).
    pub fn set_mode(&mut self, mode: ConsoleMode) {
        let old_mode = self.state.mode;
        self.state.mode = mode;
        if old_mode == ConsoleMode::Graphics && mode == ConsoleMode::Text {
            self.state.flush_fullscreen();
            self.state.draw_cursor();
        }
    }

    /// Sets a custom font.
    pub fn set_font(&mut self, font: BitmapFont) -> Result<(), ConsoleSetFontError> {
        self.state.set_font(font)
    }

    /// Sends a byte buffer to the console, processing ANSI escapes and text.
    pub fn send(&mut self, buf: &[u8]) {
        self.state.erase_cursor();
        for &byte in buf {
            if self.escape_fsm.eat(byte, &mut self.state) {
                continue;
            }
            if byte == 0 {
                continue;
            }
            self.state.send_char(byte);
        }
        self.state.draw_cursor();
    }

    /// Writes raw byte slice (alias for send).
    pub fn write_bytes(&mut self, buf: &[u8]) {
        self.send(buf);
    }

    /// Returns the number of text columns.
    pub fn cols(&self) -> usize {
        self.state.backend.width() / self.state.font.width()
    }

    /// Returns the number of text rows.
    pub fn rows(&self) -> usize {
        self.state.backend.height() / self.state.font.height()
    }

    /// Returns the screen width in pixels.
    pub fn width(&self) -> usize {
        self.state.backend.width()
    }

    /// Returns the screen height in pixels.
    pub fn height(&self) -> usize {
        self.state.backend.height()
    }

    /// Returns the character width in pixels.
    pub fn font_width(&self) -> usize {
        self.state.font.width()
    }

    /// Returns the character height in pixels.
    pub fn font_height(&self) -> usize {
        self.state.font.height()
    }

    /// Clears the entire console.
    pub fn clear(&mut self) {
        self.state.erase_cursor();
        self.state.erase_in_display(EraseInDisplay::EntireScreen);
        self.state.x_pos = 0;
        self.state.y_pos = 0;
        self.state.draw_cursor();
    }
}

impl core::fmt::Debug for FramebufferConsole {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FramebufferConsole")
            .field("cols", &self.cols())
            .field("rows", &self.rows())
            .field("mode", &self.mode())
            .finish()
    }
}

#[derive(Debug)]
struct ConsoleState {
    x_pos: usize,
    y_pos: usize,
    fg_color: Pixel,
    bg_color: Pixel,
    is_inverse: bool,
    font: BitmapFont,

    is_active: bool,
    mode: ConsoleMode,

    cursor_visible: bool,
    cursor_style: CursorStyle,
    cursor_drawn: bool,
    last_cursor_x: usize,
    last_cursor_y: usize,

    saved_x: usize,
    saved_y: usize,
    saved_fg: Pixel,
    saved_bg: Pixel,
    saved_inverse: bool,

    scroll_top: usize,
    scroll_bottom: usize,
    origin_mode: bool,
    wrap_pending: bool,
    autowrap: bool,

    bytes: Vec<u8>,
    backend: Arc<FrameBuffer>,
}

impl ConsoleState {
    fn new(backend: Arc<FrameBuffer>) -> Self {
        let buffer_size = backend.io_mem().size();
        let font = BitmapFont::new_basic();
        let max_rows = backend.height() / font.height();
        Self {
            x_pos: 0,
            y_pos: 0,
            fg_color: Pixel::WHITE,
            bg_color: Pixel::BLACK,
            is_inverse: false,
            font,
            is_active: true,
            mode: ConsoleMode::Text,
            cursor_visible: true,
            cursor_style: CursorStyle::Default,
            cursor_drawn: false,
            last_cursor_x: 0,
            last_cursor_y: 0,
            saved_x: 0,
            saved_y: 0,
            saved_fg: Pixel::WHITE,
            saved_bg: Pixel::BLACK,
            saved_inverse: false,
            scroll_top: 0,
            scroll_bottom: max_rows.saturating_sub(1),
            origin_mode: false,
            wrap_pending: false,
            autowrap: true,
            bytes: vec![0u8; buffer_size],
            backend,
        }
    }

    fn is_output_enabled(&self) -> bool {
        self.is_active && self.mode == ConsoleMode::Text
    }

    fn flush_fullscreen(&mut self) {
        if self.is_output_enabled() {
            let _ = self.backend.write_bytes_at(0, &self.bytes);
            self.cursor_drawn = false;
        }
    }

    fn draw_cursor(&mut self) {
        if !self.cursor_visible || self.cursor_drawn || !self.is_output_enabled() {
            return;
        }

        let cx = self
            .x_pos
            .min(self.backend.width().saturating_sub(self.font.width()));
        let cy = self
            .y_pos
            .min(self.backend.height().saturating_sub(self.font.height()));

        let cursor_color = if self.is_inverse {
            self.bg_color
        } else {
            self.fg_color
        };
        let cursor_pixel = self.backend.render_pixel(cursor_color);
        let pixel_size = cursor_pixel.nbytes();

        match self.cursor_style {
            CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock => {
                let cell_w = self.font.width();
                let row_bytes = cell_w * pixel_size;
                for y in cy..cy + self.font.height() {
                    let off = self.backend.calc_offset(cx, y).as_usize();
                    if off + row_bytes <= self.bytes.len() {
                        let mut buf = [0u8; 64];
                        let len = row_bytes.min(buf.len());
                        buf[..len].copy_from_slice(&self.bytes[off..off + len]);
                        for chunk in buf[..len].chunks_exact_mut(pixel_size) {
                            for b in chunk.iter_mut().take(3.min(pixel_size)) {
                                *b = !*b;
                            }
                        }
                        let _ = self.backend.write_bytes_at(off, &buf[..len]);
                    }
                }
            }
            CursorStyle::BlinkingBar | CursorStyle::SteadyBar => {
                let bar_w = 2.min(self.font.width());
                let row_bytes = bar_w * pixel_size;
                for y in cy..cy + self.font.height() {
                    let off = self.backend.calc_offset(cx, y).as_usize();
                    let mut buf = [0u8; 32];
                    let len = row_bytes.min(buf.len());
                    for chunk in buf[..len].chunks_exact_mut(pixel_size) {
                        chunk.copy_from_slice(cursor_pixel.as_slice());
                    }
                    let _ = self.backend.write_bytes_at(off, &buf[..len]);
                }
            }
            CursorStyle::Default | CursorStyle::BlinkingUnderline | CursorStyle::SteadyUnderline => {
                let cursor_height = 2.min(self.font.height());
                let cursor_y = cy + self.font.height().saturating_sub(cursor_height);
                let row_bytes = self.font.width() * pixel_size;
                for y in cursor_y..cy + self.font.height() {
                    let off = self.backend.calc_offset(cx, y).as_usize();
                    let mut buf = [0u8; 64];
                    let len = row_bytes.min(buf.len());
                    for chunk in buf[..len].chunks_exact_mut(pixel_size) {
                        chunk.copy_from_slice(cursor_pixel.as_slice());
                    }
                    let _ = self.backend.write_bytes_at(off, &buf[..len]);
                }
            }
        }

        self.last_cursor_x = cx;
        self.last_cursor_y = cy;
        self.cursor_drawn = true;
    }

    fn erase_cursor(&mut self) {
        if !self.cursor_drawn || !self.is_output_enabled() {
            return;
        }

        let cx = self.last_cursor_x;
        let cy = self.last_cursor_y;

        let pixel_size = self.backend.pixel_format().nbytes();
        let row_bytes = self.font.width() * pixel_size;

        for y in cy..cy + self.font.height() {
            let off = self.backend.calc_offset(cx, y).as_usize();
            if off + row_bytes <= self.bytes.len() {
                let _ = self
                    .backend
                    .write_bytes_at(off, &self.bytes[off..off + row_bytes]);
            }
        }

        self.cursor_drawn = false;
    }

    fn send_char(&mut self, ch: u8) {
        match ch {
            b'\n' => self.newline(),
            b'\r' => self.carriage_return(),
            b'\x08' => self.backspace(),
            b'\t' => {
                self.erase_cursor();
                self.wrap_pending = false;
                let tab_cols = 8;
                let col = self.x_pos / self.font.width();
                let next_col = ((col / tab_cols) + 1) * tab_cols;
                let max_cols = self.backend.width() / self.font.width();
                if next_col >= max_cols {
                    self.newline();
                    self.x_pos = 0;
                } else {
                    self.x_pos = next_col * self.font.width();
                }
                self.draw_cursor();
            }
            _ => {
                self.erase_cursor();
                if self.wrap_pending {
                    self.wrap_pending = false;
                    self.newline();
                    self.x_pos = 0;
                }
                self.draw_char(ch);
                let max_x = self.backend.width().saturating_sub(self.font.width());
                if self.x_pos >= max_x {
                    if self.autowrap {
                        self.wrap_pending = true;
                    }
                } else {
                    self.x_pos += self.font.width();
                }
                self.draw_cursor();
            }
        }
    }

    fn newline(&mut self) {
        self.erase_cursor();
        if self.wrap_pending {
            self.wrap_pending = false;
            self.x_pos = 0;
        }
        let top_y = self.scroll_top * self.font.height();
        let bottom_y = self.scroll_bottom * self.font.height();

        if self.y_pos >= top_y && self.y_pos <= bottom_y {
            if self.y_pos == bottom_y {
                self.scroll_region_up(1);
            } else {
                self.y_pos += self.font.height();
            }
        } else {
            let max_rows = self.backend.height() / self.font.height();
            if self.y_pos + self.font.height() < self.backend.height() {
                self.y_pos += self.font.height();
            } else if self.scroll_top == 0 && self.scroll_bottom == max_rows.saturating_sub(1) {
                self.shift_lines_up();
            }
        }
        self.draw_cursor();
    }

    fn carriage_return(&mut self) {
        self.erase_cursor();
        self.wrap_pending = false;
        self.x_pos = 0;
        self.draw_cursor();
    }

    fn backspace(&mut self) {
        self.erase_cursor();
        self.wrap_pending = false;
        if self.x_pos >= self.font.width() {
            self.x_pos -= self.font.width();
        }
        self.draw_cursor();
    }

    fn draw_char(&mut self, ch: u8) {
        let Some(font_ch) = self.font.char(ch) else {
            return;
        };

        let (fg, bg) = if self.is_inverse {
            (self.bg_color, self.fg_color)
        } else {
            (self.fg_color, self.bg_color)
        };

        let fg_pixel = self.backend.render_pixel(fg);
        let bg_pixel = self.backend.render_pixel(bg);
        let pixel_size = fg_pixel.nbytes();
        let is_output_enabled = self.is_output_enabled();

        let mut offset = self.backend.calc_offset(self.x_pos, self.y_pos);

        for row in font_ch.rows() {
            let off_st = offset.as_usize();
            let off_ed = off_st + pixel_size * self.font.width();
            if off_ed <= self.bytes.len() {
                let render_buf = &mut self.bytes[off_st..off_ed];

                let chunks = render_buf.chunks_exact_mut(pixel_size);
                for (chunk, is_fg) in chunks.zip(row.bits()) {
                    let pixel = if is_fg { fg_pixel } else { bg_pixel };
                    chunk.copy_from_slice(pixel.as_slice());
                }

                if is_output_enabled {
                    let _ = self.backend.write_bytes_at(off_st, render_buf);
                }
            }
            offset.y_add(1);
        }
    }

    fn shift_lines_up(&mut self) {
        self.erase_cursor();
        self.scroll_region_up(1);
    }

    fn scroll_region_up(&mut self, count: usize) {
        self.erase_cursor();
        let max_row = (self.backend.height() / self.font.height()).saturating_sub(1);
        let top_row = self.scroll_top.min(max_row);
        let bot_row = self.scroll_bottom.min(max_row).max(top_row);
        let num_rows = bot_row - top_row + 1;
        let count = count.min(num_rows);
        if count == 0 {
            self.draw_cursor();
            return;
        }

        let bg = if self.is_inverse {
            self.fg_color
        } else {
            self.bg_color
        };
        let top_y = top_row * self.font.height();
        let bot_y = (bot_row + 1) * self.font.height();
        let shift_y = count * self.font.height();

        let top_off = self.backend.calc_offset(0, top_y).as_usize();
        let src_off = self.backend.calc_offset(0, top_y + shift_y).as_usize();
        let bot_off = self
            .backend
            .calc_offset(0, bot_y)
            .as_usize()
            .min(self.bytes.len());

        if src_off < bot_off && top_off < self.bytes.len() {
            self.bytes.copy_within(src_off..bot_off, top_off);
        }

        let clear_top_y = (bot_row + 1 - count) * self.font.height();
        self.fill_rect_pixels(0, clear_top_y, self.backend.width(), bot_y, bg);

        if self.is_output_enabled() && top_off < bot_off {
            let _ = self
                .backend
                .write_bytes_at(top_off, &self.bytes[top_off..bot_off]);
        }
        self.draw_cursor();
    }

    fn scroll_region_down(&mut self, count: usize) {
        self.erase_cursor();
        let max_row = (self.backend.height() / self.font.height()).saturating_sub(1);
        let top_row = self.scroll_top.min(max_row);
        let bot_row = self.scroll_bottom.min(max_row).max(top_row);
        let num_rows = bot_row - top_row + 1;
        let count = count.min(num_rows);
        if count == 0 {
            self.draw_cursor();
            return;
        }

        let bg = if self.is_inverse {
            self.fg_color
        } else {
            self.bg_color
        };
        let top_y = top_row * self.font.height();
        let bot_y = (bot_row + 1) * self.font.height();
        let shift_y = count * self.font.height();

        let top_off = self.backend.calc_offset(0, top_y).as_usize();
        let dst_off = self.backend.calc_offset(0, top_y + shift_y).as_usize();
        let bot_off = self
            .backend
            .calc_offset(0, bot_y)
            .as_usize()
            .min(self.bytes.len());

        if dst_off < bot_off && top_off < self.bytes.len() {
            let copy_len = bot_off - dst_off;
            self.bytes.copy_within(top_off..top_off + copy_len, dst_off);
        }

        self.fill_rect_pixels(0, top_y, self.backend.width(), top_y + shift_y, bg);

        if self.is_output_enabled() && top_off < bot_off {
            let _ = self
                .backend
                .write_bytes_at(top_off, &self.bytes[top_off..bot_off]);
        }
        self.draw_cursor();
    }

    fn delete_lines_internal(&mut self, count: usize) {
        let cur_row = self.y_pos / self.font.height();
        let max_row = (self.backend.height() / self.font.height()).saturating_sub(1);
        let top_row = self.scroll_top.min(max_row);
        let bot_row = self.scroll_bottom.min(max_row).max(top_row);

        if cur_row < top_row || cur_row > bot_row {
            return;
        }

        let num_rows = bot_row - cur_row + 1;
        let count = count.min(num_rows);
        if count == 0 {
            return;
        }

        let bg = if self.is_inverse {
            self.fg_color
        } else {
            self.bg_color
        };
        let top_y = cur_row * self.font.height();
        let bot_y = (bot_row + 1) * self.font.height();
        let shift_y = count * self.font.height();

        let top_off = self.backend.calc_offset(0, top_y).as_usize();
        let src_off = self.backend.calc_offset(0, top_y + shift_y).as_usize();
        let bot_off = self
            .backend
            .calc_offset(0, bot_y)
            .as_usize()
            .min(self.bytes.len());

        if src_off < bot_off && top_off < self.bytes.len() {
            self.bytes.copy_within(src_off..bot_off, top_off);
        }

        let clear_top_y = (bot_row + 1 - count) * self.font.height();
        self.fill_rect_pixels(0, clear_top_y, self.backend.width(), bot_y, bg);

        if self.is_output_enabled() && top_off < bot_off {
            let _ = self
                .backend
                .write_bytes_at(top_off, &self.bytes[top_off..bot_off]);
        }
    }

    fn insert_lines_internal(&mut self, count: usize) {
        let cur_row = self.y_pos / self.font.height();
        let max_row = (self.backend.height() / self.font.height()).saturating_sub(1);
        let top_row = self.scroll_top.min(max_row);
        let bot_row = self.scroll_bottom.min(max_row).max(top_row);

        if cur_row < top_row || cur_row > bot_row {
            return;
        }

        let num_rows = bot_row - cur_row + 1;
        let count = count.min(num_rows);
        if count == 0 {
            return;
        }

        let bg = if self.is_inverse {
            self.fg_color
        } else {
            self.bg_color
        };
        let top_y = cur_row * self.font.height();
        let bot_y = (bot_row + 1) * self.font.height();
        let shift_y = count * self.font.height();

        let top_off = self.backend.calc_offset(0, top_y).as_usize();
        let dst_off = self.backend.calc_offset(0, top_y + shift_y).as_usize();
        let bot_off = self
            .backend
            .calc_offset(0, bot_y)
            .as_usize()
            .min(self.bytes.len());

        if dst_off < bot_off && top_off < self.bytes.len() {
            let copy_len = bot_off - dst_off;
            self.bytes.copy_within(top_off..top_off + copy_len, dst_off);
        }

        self.fill_rect_pixels(0, top_y, self.backend.width(), top_y + shift_y, bg);

        if self.is_output_enabled() && top_off < bot_off {
            let _ = self
                .backend
                .write_bytes_at(top_off, &self.bytes[top_off..bot_off]);
        }
    }

    fn delete_chars_internal(&mut self, count: usize) {
        let max_cols = self.backend.width() / self.font.width();
        let cur_col = self.x_pos / self.font.width();
        if cur_col >= max_cols {
            return;
        }
        let count = count.min(max_cols - cur_col);
        if count == 0 {
            return;
        }

        let bg = if self.is_inverse {
            self.fg_color
        } else {
            self.bg_color
        };
        let pixel_size = self.backend.pixel_format().nbytes();
        let row_w = self.backend.width();
        let shift_bytes = count * self.font.width() * pixel_size;
        let cy0 = self.y_pos;
        let cy1 = (cy0 + self.font.height()).min(self.backend.height());

        for y in cy0..cy1 {
            let line_st = self.backend.calc_offset(0, y).as_usize();
            let cur_st = line_st + self.x_pos * pixel_size;
            let src_st = cur_st + shift_bytes;
            let line_ed = line_st + row_w * pixel_size;

            if src_st < line_ed && line_ed <= self.bytes.len() {
                self.bytes.copy_within(src_st..line_ed, cur_st);
            }
        }

        let clear_x0 = (max_cols - count) * self.font.width();
        self.fill_rect_pixels(clear_x0, cy0, row_w, cy1, bg);
        if self.is_output_enabled() {
            for y in cy0..cy1 {
                let cur_st = self.backend.calc_offset(self.x_pos, y).as_usize();
                let line_ed = self
                    .backend
                    .calc_offset(row_w, y)
                    .as_usize()
                    .min(self.bytes.len());
                if cur_st < line_ed {
                    let _ = self
                        .backend
                        .write_bytes_at(cur_st, &self.bytes[cur_st..line_ed]);
                }
            }
        }
    }

    fn insert_chars_internal(&mut self, count: usize) {
        let max_cols = self.backend.width() / self.font.width();
        let cur_col = self.x_pos / self.font.width();
        if cur_col >= max_cols {
            return;
        }
        let count = count.min(max_cols - cur_col);
        if count == 0 {
            return;
        }

        let bg = if self.is_inverse {
            self.fg_color
        } else {
            self.bg_color
        };
        let pixel_size = self.backend.pixel_format().nbytes();
        let row_w = self.backend.width();
        let shift_bytes = count * self.font.width() * pixel_size;
        let cy0 = self.y_pos;
        let cy1 = (cy0 + self.font.height()).min(self.backend.height());

        for y in cy0..cy1 {
            let line_st = self.backend.calc_offset(0, y).as_usize();
            let cur_st = line_st + self.x_pos * pixel_size;
            let dst_st = cur_st + shift_bytes;
            let line_ed = line_st + row_w * pixel_size;

            if dst_st < line_ed && line_ed <= self.bytes.len() {
                let copy_len = line_ed - dst_st;
                self.bytes.copy_within(cur_st..cur_st + copy_len, dst_st);
            }
        }

        let clear_x1 = (cur_col + count) * self.font.width();
        self.fill_rect_pixels(self.x_pos, cy0, clear_x1, cy1, bg);
        if self.is_output_enabled() {
            for y in cy0..cy1 {
                let cur_st = self.backend.calc_offset(self.x_pos, y).as_usize();
                let line_ed = self
                    .backend
                    .calc_offset(row_w, y)
                    .as_usize()
                    .min(self.bytes.len());
                if cur_st < line_ed {
                    let _ = self
                        .backend
                        .write_bytes_at(cur_st, &self.bytes[cur_st..line_ed]);
                }
            }
        }
    }

    fn erase_chars_internal(&mut self, count: usize) {
        let bg = if self.is_inverse {
            self.fg_color
        } else {
            self.bg_color
        };
        let cy0 = self.y_pos;
        let cy1 = (cy0 + self.font.height()).min(self.backend.height());
        let x_len = count * self.font.width();
        let x1 = (self.x_pos + x_len).min(self.backend.width());
        self.fill_rect_pixels(self.x_pos, cy0, x1, cy1, bg);
    }

    fn set_font(&mut self, font: BitmapFont) -> Result<(), ConsoleSetFontError> {
        if font.width() > self.backend.width() || font.height() > self.backend.height() / 2 {
            return Err(ConsoleSetFontError::InvalidFont);
        }
        self.erase_cursor();
        self.font = font;
        if self.y_pos + self.font.height() > self.backend.height() {
            self.shift_lines_up();
        }
        self.draw_cursor();
        Ok(())
    }

    fn fill_rect_pixels(&mut self, x0: usize, y0: usize, x1: usize, y1: usize, color: Pixel) {
        let x0 = x0.min(self.backend.width());
        let x1 = x1.min(self.backend.width());
        let y0 = y0.min(self.backend.height());
        let y1 = y1.min(self.backend.height());

        if x0 >= x1 || y0 >= y1 {
            return;
        }

        let rendered_pixel = self.backend.render_pixel(color);
        let pixel_size = rendered_pixel.nbytes();
        let row_bytes = (x1 - x0) * pixel_size;
        let is_output_enabled = self.is_output_enabled();

        for y in y0..y1 {
            let off = self.backend.calc_offset(x0, y).as_usize();
            if off + row_bytes <= self.bytes.len() {
                let buf = &mut self.bytes[off..off + row_bytes];
                for chunk in buf.chunks_exact_mut(pixel_size) {
                    chunk.copy_from_slice(rendered_pixel.as_slice());
                }
                if is_output_enabled {
                    let _ = self.backend.write_bytes_at(off, buf);
                }
            }
        }
    }

    fn cursor_cell_rect(&self) -> (usize, usize, usize, usize) {
        let cx0 = self.x_pos;
        let cx1 = (cx0 + self.font.width()).min(self.backend.width());
        let cy0 = self.y_pos;
        let cy1 = (cy0 + self.font.height()).min(self.backend.height());
        (cx0, cy0, cx1, cy1)
    }
}

impl EscapeOp for ConsoleState {
    fn set_cursor(&mut self, col: usize, row: usize) {
        self.erase_cursor();
        self.wrap_pending = false;
        let max_col = (self.backend.width() / self.font.width()).saturating_sub(1);
        let max_row = (self.backend.height() / self.font.height()).saturating_sub(1);

        let actual_row = if self.origin_mode {
            (self.scroll_top + row).min(self.scroll_bottom)
        } else {
            row.min(max_row)
        };

        self.x_pos = col.min(max_col) * self.font.width();
        self.y_pos = actual_row * self.font.height();
        self.draw_cursor();
    }

    fn move_cursor(&mut self, dx: isize, dy: isize) {
        self.erase_cursor();
        self.wrap_pending = false;
        let current_col = (self.x_pos / self.font.width()) as isize;
        let current_row = (self.y_pos / self.font.height()) as isize;
        let max_col = (self.backend.width() / self.font.width()).saturating_sub(1) as isize;
        let max_row = (self.backend.height() / self.font.height()).saturating_sub(1) as isize;

        let (min_row, max_allowed_row) = if self.origin_mode {
            (self.scroll_top as isize, self.scroll_bottom as isize)
        } else {
            (0, max_row)
        };

        let new_col = (current_col + dx).clamp(0, max_col) as usize;
        let new_row = (current_row + dy).clamp(min_row, max_allowed_row) as usize;

        self.x_pos = new_col * self.font.width();
        self.y_pos = new_row * self.font.height();
        self.draw_cursor();
    }

    fn set_cursor_col(&mut self, col: usize) {
        self.erase_cursor();
        self.wrap_pending = false;
        let max_col = (self.backend.width() / self.font.width()).saturating_sub(1);
        self.x_pos = col.min(max_col) * self.font.width();
        self.draw_cursor();
    }

    fn set_cursor_row(&mut self, row: usize) {
        self.erase_cursor();
        self.wrap_pending = false;
        let max_row = (self.backend.height() / self.font.height()).saturating_sub(1);
        let actual_row = if self.origin_mode {
            (self.scroll_top + row).min(self.scroll_bottom)
        } else {
            row.min(max_row)
        };
        self.y_pos = actual_row * self.font.height();
        self.draw_cursor();
    }

    fn save_cursor(&mut self) {
        self.saved_x = self.x_pos;
        self.saved_y = self.y_pos;
        self.saved_fg = self.fg_color;
        self.saved_bg = self.bg_color;
        self.saved_inverse = self.is_inverse;
    }

    fn restore_cursor(&mut self) {
        self.erase_cursor();
        self.wrap_pending = false;
        let max_x = self.backend.width().saturating_sub(self.font.width());
        let max_y = self.backend.height().saturating_sub(self.font.height());

        self.x_pos = self.saved_x.min(max_x);
        self.y_pos = self.saved_y.min(max_y);
        self.fg_color = self.saved_fg;
        self.bg_color = self.saved_bg;
        self.is_inverse = self.saved_inverse;
        self.draw_cursor();
    }

    fn set_origin_mode(&mut self, mode: bool) {
        self.origin_mode = mode;
        self.set_cursor(0, 0);
    }

    fn set_cursor_visible(&mut self, visible: bool) {
        if self.cursor_visible != visible {
            if !visible {
                self.erase_cursor();
                self.cursor_visible = false;
            } else {
                self.cursor_visible = true;
                self.draw_cursor();
            }
        }
    }

    fn set_cursor_key_mode(&mut self, mode: bool) {
        tty::set_application_cursor_keys(mode);
    }

    fn set_autowrap(&mut self, mode: bool) {
        self.autowrap = mode;
        if !mode {
            self.wrap_pending = false;
        }
    }

    fn set_inverse(&mut self, val: bool) {
        self.is_inverse = val;
    }

    fn reset(&mut self) {
        self.erase_cursor();
        self.fg_color = Pixel::WHITE;
        self.bg_color = Pixel::BLACK;
        self.is_inverse = false;
        self.cursor_visible = true;
        self.cursor_style = CursorStyle::Default;
        self.autowrap = true;
        self.wrap_pending = false;
        self.saved_x = 0;
        self.saved_y = 0;
        self.saved_fg = Pixel::WHITE;
        self.saved_bg = Pixel::BLACK;
        self.saved_inverse = false;
        let max_rows = self.backend.height() / self.font.height();
        self.scroll_top = 0;
        self.scroll_bottom = max_rows.saturating_sub(1);
        self.erase_in_display(EraseInDisplay::EntireScreen);
        self.x_pos = 0;
        self.y_pos = 0;
        self.draw_cursor();
    }

    fn set_fg_color(&mut self, val: Pixel) {
        self.fg_color = val;
    }

    fn set_bg_color(&mut self, val: Pixel) {
        self.bg_color = val;
    }

    fn erase_in_display(&mut self, mode: EraseInDisplay) {
        self.erase_cursor();
        self.wrap_pending = false;
        let bg = if self.is_inverse {
            self.fg_color
        } else {
            self.bg_color
        };
        let w = self.backend.width();
        let h = self.backend.height();
        let (cx0, cy0, cx1, cy1) = self.cursor_cell_rect();

        match mode {
            EraseInDisplay::CursorToEnd => {
                self.fill_rect_pixels(cx0, cy0, w, cy1, bg);
                if cy1 < h {
                    self.fill_rect_pixels(0, cy1, w, h, bg);
                }
            }
            EraseInDisplay::CursorToBeginning => {
                if cy0 > 0 {
                    self.fill_rect_pixels(0, 0, w, cy0, bg);
                }
                self.fill_rect_pixels(0, cy0, cx1, cy1, bg);
            }
            EraseInDisplay::EntireScreen | EraseInDisplay::EntireScreenAndScrollback => {
                self.fill_rect_pixels(0, 0, w, h, bg);
            }
        }
        self.draw_cursor();
    }

    fn erase_in_line(&mut self, mode: EraseInLine) {
        self.erase_cursor();
        self.wrap_pending = false;
        let bg = if self.is_inverse {
            self.fg_color
        } else {
            self.bg_color
        };
        let w = self.backend.width();
        let (_, cy0, cx1, cy1) = self.cursor_cell_rect();

        match mode {
            EraseInLine::CursorToEnd => {
                self.fill_rect_pixels(self.x_pos, cy0, w, cy1, bg);
            }
            EraseInLine::CursorToBeginning => {
                self.fill_rect_pixels(0, cy0, cx1, cy1, bg);
            }
            EraseInLine::EntireLine => {
                self.fill_rect_pixels(0, cy0, w, cy1, bg);
            }
        }
        self.draw_cursor();
    }

    fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        self.erase_cursor();
        self.wrap_pending = false;
        let max_rows = self.backend.height() / self.font.height();
        if top == 0 && bottom == 0 {
            self.scroll_top = 0;
            self.scroll_bottom = max_rows.saturating_sub(1);
        } else {
            let top_0b = top.saturating_sub(1).min(max_rows.saturating_sub(1));
            let bot_0b = if bottom == 0 || bottom > max_rows {
                max_rows.saturating_sub(1)
            } else {
                bottom.saturating_sub(1)
            };
            self.scroll_top = top_0b.min(bot_0b);
            self.scroll_bottom = bot_0b.max(top_0b);
        }
        self.x_pos = 0;
        self.y_pos = if self.origin_mode {
            self.scroll_top * self.font.height()
        } else {
            0
        };
        self.draw_cursor();
    }

    fn delete_lines(&mut self, count: usize) {
        self.wrap_pending = false;
        self.erase_cursor();
        self.delete_lines_internal(count);
        self.draw_cursor();
    }

    fn insert_lines(&mut self, count: usize) {
        self.wrap_pending = false;
        self.erase_cursor();
        self.insert_lines_internal(count);
        self.draw_cursor();
    }

    fn delete_chars(&mut self, count: usize) {
        self.wrap_pending = false;
        self.erase_cursor();
        self.delete_chars_internal(count);
        self.draw_cursor();
    }

    fn insert_chars(&mut self, count: usize) {
        self.wrap_pending = false;
        self.erase_cursor();
        self.insert_chars_internal(count);
        self.draw_cursor();
    }

    fn erase_chars(&mut self, count: usize) {
        self.wrap_pending = false;
        self.erase_cursor();
        self.erase_chars_internal(count);
        self.draw_cursor();
    }

    fn report_cursor_position(&mut self) {
        let row = (self.y_pos / self.font.height()) + 1;
        let col = (self.x_pos / self.font.width()) + 1;
        let resp = alloc::format!("\x1B[{};{}R", row, col);
        for b in resp.bytes() {
            tty::queue_input(b);
        }
    }

    fn reverse_index(&mut self) {
        self.wrap_pending = false;
        self.erase_cursor();
        let top_y = self.scroll_top * self.font.height();
        let bottom_y = self.scroll_bottom * self.font.height();

        if self.y_pos >= top_y && self.y_pos <= bottom_y {
            if self.y_pos == top_y {
                self.scroll_region_down(1);
            } else {
                self.y_pos = self.y_pos.saturating_sub(self.font.height());
            }
        } else if self.y_pos >= self.font.height() {
            self.y_pos -= self.font.height();
        }
        self.draw_cursor();
    }

    fn index(&mut self) {
        self.wrap_pending = false;
        self.erase_cursor();
        let top_y = self.scroll_top * self.font.height();
        let bottom_y = self.scroll_bottom * self.font.height();

        if self.y_pos >= top_y && self.y_pos <= bottom_y {
            if self.y_pos == bottom_y {
                self.scroll_region_up(1);
            } else {
                self.y_pos += self.font.height();
            }
        } else if self.y_pos + self.font.height() < self.backend.height() {
            self.y_pos += self.font.height();
        } else {
            self.scroll_region_up(1);
        }
        self.draw_cursor();
    }

    fn next_line(&mut self) {
        self.wrap_pending = false;
        self.x_pos = 0;
        self.index();
    }

    fn scroll_up(&mut self, count: usize) {
        self.wrap_pending = false;
        self.erase_cursor();
        self.scroll_region_up(count);
    }

    fn scroll_down(&mut self, count: usize) {
        self.wrap_pending = false;
        self.erase_cursor();
        self.scroll_region_down(count);
    }

    fn set_cursor_style(&mut self, style: CursorStyle) {
        if self.cursor_style != style {
            self.erase_cursor();
            self.cursor_style = style;
            self.draw_cursor();
        }
    }
}
