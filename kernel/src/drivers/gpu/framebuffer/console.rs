//! Framebuffer Text Console (fbcon)
//!
//! Provides a text console over the linear framebuffer with ANSI escape
//! sequence parsing, scrolling, color management, and cursor control.

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
    ansi_state: AnsiState,
    ansi_params: [u32; 8],
    ansi_param_count: usize,
    ansi_current_param: u32,
    ansi_has_param: bool,
}

impl FbConsole {
    /// Creates a new FbConsole for the given framebuffer dimensions.
    pub fn new(fb_width: usize, fb_height: usize) -> Self {
        let cols = fb_width / FONT_WIDTH;
        let rows = fb_height / FONT_HEIGHT;

        Self {
            cursor_x: 0,
            cursor_y: 0,
            cols: if cols > 0 { cols } else { 80 },
            rows: if rows > 0 { rows } else { 25 },
            fg: Color::WHITE,
            bg: Color::rgb(15, 15, 20),
            default_fg: Color::WHITE,
            default_bg: Color::rgb(15, 15, 20),
            ansi_state: AnsiState::Normal,
            ansi_params: [0; 8],
            ansi_param_count: 0,
            ansi_current_param: 0,
            ansi_has_param: false,
        }
    }

    /// Clears the text console and resets cursor position to top-left.
    pub fn clear(&mut self) {
        if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
            fb.clear(self.bg);
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    /// Scrolls the console buffer upwards by one row.
    pub fn scroll(&mut self) {
        if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
            fb.scroll_up(FONT_HEIGHT, self.bg);
        }
    }

    /// Outputs a single byte to the console.
    pub fn write_byte(&mut self, byte: u8) {
        match self.ansi_state {
            AnsiState::Normal => {
                if byte == 0x1B {
                    // Escape byte
                    self.ansi_state = AnsiState::Esc;
                } else {
                    self.handle_char(byte as char);
                }
            }
            AnsiState::Esc => {
                if byte == b'[' {
                    self.ansi_state = AnsiState::Bracket;
                    self.ansi_params = [0; 8];
                    self.ansi_param_count = 0;
                    self.ansi_current_param = 0;
                    self.ansi_has_param = false;
                } else {
                    self.ansi_state = AnsiState::Normal;
                    self.handle_char(byte as char);
                }
            }
            AnsiState::Bracket => {
                if byte.is_ascii_digit() {
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
                }
            }
        }
    }

    /// Process ANSI escape command code.
    fn apply_ansi_command(&mut self, cmd: u8) {
        match cmd {
            b'm' => {
                // Select Graphic Rendition (SGR)
                if self.ansi_param_count == 0 {
                    self.fg = self.default_fg;
                    self.bg = self.default_bg;
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
                if mode == 2 {
                    self.clear();
                }
            }
            b'K' => {
                // Erase in line
                if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
                    let pixel_x = self.cursor_x * FONT_WIDTH;
                    let pixel_y = self.cursor_y * FONT_HEIGHT;
                    if pixel_x < fb.width() {
                        fb.fill_rect(pixel_x, pixel_y, fb.width() - pixel_x, FONT_HEIGHT, self.bg);
                    }
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
            _ => {}
        }
    }

    /// Handles standard printable character and control codes.
    fn handle_char(&mut self, c: char) {
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
                if (ch as u32) >= 0x20 || ch == '\0' {
                    let pixel_x = self.cursor_x * FONT_WIDTH;
                    let pixel_y = self.cursor_y * FONT_HEIGHT;

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
