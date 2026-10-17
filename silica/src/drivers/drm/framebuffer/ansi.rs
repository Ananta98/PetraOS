// SPDX-License-Identifier: MPL-2.0

//! ANSI Escape Sequence Parser.
//!
//! Based on Asterinas framebuffer ansi_escape module.

use super::pixel::Pixel;

/// A finite-state machine (FSM) to handle ANSI escape sequences.
#[derive(Debug)]
pub struct EscapeFsm {
    state: WaitFor,
    params: [Option<u32>; MAX_PARAMS],
}

/// The mode for "Erase in Display" (ED) commands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EraseInDisplay {
    CursorToEnd,
    CursorToBeginning,
    EntireScreen,
    EntireScreenAndScrollback,
}

/// The mode for "Erase in Line" (EL) commands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EraseInLine {
    CursorToEnd,
    CursorToBeginning,
    EntireLine,
}

/// The style and shape of the text cursor (DECSCUSR).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorStyle {
    Default,
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

/// A trait to execute operations from ANSI escape sequences.
pub trait EscapeOp {
    /// Sets the cursor position (0-indexed: x = col, y = row).
    fn set_cursor(&mut self, x: usize, y: usize);

    /// Moves the cursor relative to current position.
    fn move_cursor(&mut self, _dx: isize, _dy: isize) {}

    /// Sets the cursor column (0-indexed).
    fn set_cursor_col(&mut self, _col: usize) {}

    /// Sets the cursor row (0-indexed).
    fn set_cursor_row(&mut self, _row: usize) {}

    /// Saves the current cursor position and attributes (DECSC / SCO).
    fn save_cursor(&mut self) {}

    /// Restores the previously saved cursor position and attributes (DECRC / SCO).
    fn restore_cursor(&mut self) {}

    /// Shows or hides the cursor (DECTCEM).
    fn set_cursor_visible(&mut self, _visible: bool) {}

    /// Enables or disables DEC Application Cursor Keys mode (DECCKM: CSI ? 1 h / l).
    fn set_cursor_key_mode(&mut self, _mode: bool) {}

    /// Enables or disables DEC Auto-wrap Mode (DECAWM: CSI ? 7 h / l).
    fn set_autowrap(&mut self, _mode: bool) {}

    /// Sets or resets origin mode (DECOM: CSI ? 6 h / l).
    fn set_origin_mode(&mut self, _mode: bool) {}

    /// Sets reverse video mode (SGR 7 / 27).
    fn set_inverse(&mut self, _val: bool) {}

    /// Resets console style and cursor to initial state (RIS).
    fn reset(&mut self) {}

    /// Sets the foreground color.
    fn set_fg_color(&mut self, val: Pixel);

    /// Sets the background color.
    fn set_bg_color(&mut self, val: Pixel);

    /// Erases part or all of the display.
    fn erase_in_display(&mut self, mode: EraseInDisplay);

    /// Erases part or all of the current line.
    fn erase_in_line(&mut self, _mode: EraseInLine) {}

    /// Sets top and bottom scrolling margins (DECSTBM: CSI Pt ; Pb r). 1-indexed.
    fn set_scroll_region(&mut self, _top: usize, _bottom: usize) {}

    /// Deletes n lines starting from current line (DL: CSI n M).
    fn delete_lines(&mut self, _count: usize) {}

    /// Inserts n blank lines at current line (IL: CSI n L).
    fn insert_lines(&mut self, _count: usize) {}

    /// Deletes n characters from current line (DCH: CSI n P).
    fn delete_chars(&mut self, _count: usize) {}

    /// Inserts n blank characters into current line (ICH: CSI n @).
    fn insert_chars(&mut self, _count: usize) {}

    /// Erases n characters from cursor without moving cursor (ECH: CSI n X).
    fn erase_chars(&mut self, _count: usize) {}

    /// Reports cursor position to input queue (DSR: CSI 6 n).
    fn report_cursor_position(&mut self) {}

    /// Reverse index (RI: ESC M). Scrolls down if at top margin.
    fn reverse_index(&mut self) {}

    /// Forward index (IND: ESC D). Scrolls up if at bottom margin.
    fn index(&mut self) {}

    /// Next line (NEL: ESC E). Moves to column 0 and performs forward index.
    fn next_line(&mut self) {}

    /// Scrolls the active scrolling region up by n lines (SU: CSI n S).
    fn scroll_up(&mut self, _count: usize) {}

    /// Scrolls the active scrolling region down by n lines (SD: CSI n T).
    fn scroll_down(&mut self, _count: usize) {}

    /// Sets the cursor shape/style (DECSCUSR: CSI Ps SP q or CSI Ps q).
    fn set_cursor_style(&mut self, _style: CursorStyle) {}
}

const MAX_PARAMS: usize = 8;
const MAX_OSC_LEN: u8 = 128;

#[derive(Clone, Copy, Debug)]
enum WaitFor {
    Escape,
    EscapeSeq,
    Charset,
    Csi {
        idx: u8,
        is_private: bool,
        in_intermediate: bool,
    },
    Osc {
        len: u8,
        is_last_esc: bool,
    },
}

#[rustfmt::skip]
const COLORS: [Pixel; 16] = [
    // Black
    Pixel { red: 0, green: 0, blue: 0 },
    // Red
    Pixel { red: 170, green: 0, blue: 0 },
    // Green
    Pixel { red: 0, green: 170, blue: 0 },
    // Yellow
    Pixel { red: 170, green: 85, blue: 0 },
    // Blue
    Pixel { red: 0, green: 0, blue: 170 },
    // Magenta
    Pixel { red: 170, green: 0, blue: 170 },
    // Cyan
    Pixel { red: 0, green: 170, blue: 170 },
    // White
    Pixel { red: 170, green: 170, blue: 170 },
    // Bright Black (Gray)
    Pixel { red: 85, green: 85, blue: 85 },
    // Bright Red
    Pixel { red: 255, green: 85, blue: 85 },
    // Bright Green
    Pixel { red: 85, green: 255, blue: 85 },
    // Bright Yellow
    Pixel { red: 255, green: 255, blue: 85 },
    // Bright Blue
    Pixel { red: 85, green: 85, blue: 255 },
    // Bright Magenta
    Pixel { red: 255, green: 85, blue: 255 },
    // Bright Cyan
    Pixel { red: 85, green: 255, blue: 255 },
    // Bright White
    Pixel { red: 255, green: 255, blue: 255 },
];

impl EscapeFsm {
    const ESC: u8 = 0x1B;
    const BEL: u8 = 0x07;

    pub fn new() -> Self {
        Self {
            state: WaitFor::Escape,
            params: [None; MAX_PARAMS],
        }
    }

    /// Tries to eat a byte as part of the ANSI escape sequence.
    /// Returns `true` if consumed by the FSM; `false` if printable text.
    pub fn eat<T: EscapeOp>(&mut self, byte: u8, op: &mut T) -> bool {
        match self.state {
            WaitFor::Escape => {
                if byte == Self::ESC {
                    self.state = WaitFor::EscapeSeq;
                    return true;
                }
                false
            }

            WaitFor::EscapeSeq => {
                match byte {
                    b'[' => {
                        self.params.fill(None);
                        self.state = WaitFor::Csi {
                            idx: 0,
                            is_private: false,
                            in_intermediate: false,
                        };
                    }
                    b']' => {
                        self.state = WaitFor::Osc {
                            len: 0,
                            is_last_esc: false,
                        };
                    }
                    b'7' => {
                        // DECSC - DEC Save Cursor (used by apk progress bar)
                        op.save_cursor();
                        self.state = WaitFor::Escape;
                    }
                    b'8' => {
                        // DECRC - DEC Restore Cursor (used by apk progress bar)
                        op.restore_cursor();
                        self.state = WaitFor::Escape;
                    }
                    b'M' => {
                        // RI - Reverse Index
                        op.reverse_index();
                        self.state = WaitFor::Escape;
                    }
                    b'D' => {
                        // IND - Index
                        op.index();
                        self.state = WaitFor::Escape;
                    }
                    b'E' => {
                        // NEL - Next Line
                        op.next_line();
                        self.state = WaitFor::Escape;
                    }
                    b'H' => {
                        // HTS - Horizontal Tabulation Set (ignored)
                        self.state = WaitFor::Escape;
                    }
                    b'c' => {
                        // RIS - Reset Initial State
                        op.reset();
                        self.state = WaitFor::Escape;
                    }
                    b'=' => {
                        // DECKPAM - Application Keypad
                        self.state = WaitFor::Escape;
                    }
                    b'>' => {
                        // DECKPNM - Normal Keypad
                        self.state = WaitFor::Escape;
                    }
                    b'(' | b')' => {
                        // Charset designation (e.g. \e(B for ASCII)
                        self.state = WaitFor::Charset;
                    }
                    Self::ESC => {
                        self.state = WaitFor::EscapeSeq;
                    }
                    _ => self.state = WaitFor::Escape,
                }
                true
            }

            WaitFor::Charset => {
                self.state = WaitFor::Escape;
                true
            }

            WaitFor::Csi {
                idx,
                is_private,
                in_intermediate,
            } => {
                self.parse_csi(byte, idx, is_private, in_intermediate, op);
                true
            }

            WaitFor::Osc { len, is_last_esc } => {
                match byte {
                    Self::BEL => self.state = WaitFor::Escape,
                    b'\\' if is_last_esc => self.state = WaitFor::Escape,
                    _ => {
                        if len > MAX_OSC_LEN {
                            self.state = WaitFor::Escape;
                            return false;
                        }
                        self.state = WaitFor::Osc {
                            len: len + 1,
                            is_last_esc: byte == Self::ESC,
                        };
                    }
                }
                true
            }
        }
    }

    fn parse_csi<T: EscapeOp>(
        &mut self,
        byte: u8,
        idx: u8,
        is_private: bool,
        in_intermediate: bool,
        op: &mut T,
    ) {
        match byte {
            0x20..=0x2F => {
                self.state = WaitFor::Csi {
                    idx,
                    is_private,
                    in_intermediate: true,
                };
            }

            0x30..=0x3F if !in_intermediate => match byte {
                b'0'..=b'9' => {
                    let i = idx as usize;
                    if i < MAX_PARAMS {
                        let p = &mut self.params[i];
                        *p = Some(
                            p.unwrap_or(0)
                                .saturating_mul(10)
                                .saturating_add((byte - b'0') as u32),
                        );
                    }
                    self.state = WaitFor::Csi {
                        idx,
                        is_private,
                        in_intermediate: false,
                    };
                }

                b';' => {
                    let next = idx + 1;
                    if (next as usize) < MAX_PARAMS {
                        self.state = WaitFor::Csi {
                            idx: next,
                            is_private,
                            in_intermediate: false,
                        };
                    } else {
                        self.state = WaitFor::Escape;
                    }
                }

                b':' => self.state = WaitFor::Escape,

                b'<' | b'=' | b'>' | b'?' => {
                    self.state = WaitFor::Csi {
                        idx,
                        is_private: true,
                        in_intermediate: false,
                    };
                }

                _ => unreachable!(),
            },

            0x30..=0x3F if in_intermediate => {
                self.state = WaitFor::Escape;
            }

            0x40..=0x7E => {
                self.state = WaitFor::Escape;
                let num_params = (idx as usize) + 1;
                self.handle_csi(byte, num_params, is_private, in_intermediate, op);
            }

            _ => {
                self.state = WaitFor::Escape;
            }
        }
    }

    fn handle_csi<T: EscapeOp>(
        &self,
        final_byte: u8,
        num_params: usize,
        is_private: bool,
        _in_intermediate: bool,
        op: &mut T,
    ) {
        if is_private {
            match final_byte {
                b'h' => {
                    for i in 0..num_params {
                        match self.param_or(i, 0) {
                            1 => op.set_cursor_key_mode(true),
                            6 => op.set_origin_mode(true),
                            7 => op.set_autowrap(true),
                            25 => op.set_cursor_visible(true),
                            _ => {}
                        }
                    }
                }
                b'l' => {
                    for i in 0..num_params {
                        match self.param_or(i, 0) {
                            1 => op.set_cursor_key_mode(false),
                            6 => op.set_origin_mode(false),
                            7 => op.set_autowrap(false),
                            25 => op.set_cursor_visible(false),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            return;
        }

        match final_byte {
            // CUP - Cursor Position: CSI n ; m H
            b'H' | b'f' => {
                let row_1b = self.param_or_1(0);
                let col_1b = self.param_or_1(1);
                op.set_cursor(
                    col_1b.saturating_sub(1) as usize,
                    row_1b.saturating_sub(1) as usize,
                );
            }

            // Cursor movements
            b'A' => {
                let count = self.param_or_1(0) as isize;
                op.move_cursor(0, -count);
            }
            b'B' | b'e' => {
                let count = self.param_or_1(0) as isize;
                op.move_cursor(0, count);
            }
            b'C' | b'a' => {
                let count = self.param_or_1(0) as isize;
                op.move_cursor(count, 0);
            }
            b'D' => {
                let count = self.param_or_1(0) as isize;
                op.move_cursor(-count, 0);
            }

            // CNL - Cursor Next Line: CSI n E
            b'E' => {
                let count = self.param_or_1(0) as isize;
                op.set_cursor_col(0);
                op.move_cursor(0, count);
            }

            // CPL - Cursor Previous Line: CSI n F
            b'F' => {
                let count = self.param_or_1(0) as isize;
                op.set_cursor_col(0);
                op.move_cursor(0, -count);
            }

            // CHA - Cursor Horizontal Absolute: CSI n G / '
            b'G' | b'\'' => {
                let col_1b = self.param_or_1(0);
                op.set_cursor_col(col_1b.saturating_sub(1) as usize);
            }

            // VPA - Line Position Absolute: CSI n d
            b'd' => {
                let row_1b = self.param_or_1(0);
                op.set_cursor_row(row_1b.saturating_sub(1) as usize);
            }

            // Save / Restore Cursor (SCO: CSI s / CSI u)
            b's' => op.save_cursor(),
            b'u' => op.restore_cursor(),

            // ED - Erase in Display: CSI n J
            b'J' => {
                let n = self.param_or(0, 0);
                let mode = match n {
                    0 => EraseInDisplay::CursorToEnd,
                    1 => EraseInDisplay::CursorToBeginning,
                    2 => EraseInDisplay::EntireScreen,
                    3 => EraseInDisplay::EntireScreenAndScrollback,
                    _ => return,
                };
                op.erase_in_display(mode);
            }

            // EL - Erase in Line: CSI n K
            b'K' => {
                let n = self.param_or(0, 0);
                let mode = match n {
                    0 => EraseInLine::CursorToEnd,
                    1 => EraseInLine::CursorToBeginning,
                    2 => EraseInLine::EntireLine,
                    _ => return,
                };
                op.erase_in_line(mode);
            }

            // SGR - Select Graphic Rendition
            b'm' => self.handle_sgr(num_params, op),

            // CSR - Change Scrolling Region: CSI Pt ; Pb r
            b'r' => {
                let top_1b = self.param_or(0, 1) as usize;
                let bottom_1b = self.param_or(1, 0) as usize;
                op.set_scroll_region(top_1b, bottom_1b);
            }

            // DL - Delete Lines: CSI n M
            b'M' => {
                let count = self.param_or_1(0) as usize;
                op.delete_lines(count);
            }

            // IL - Insert Lines: CSI n L
            b'L' => {
                let count = self.param_or_1(0) as usize;
                op.insert_lines(count);
            }

            // DCH - Delete Characters: CSI n P
            b'P' => {
                let count = self.param_or_1(0) as usize;
                op.delete_chars(count);
            }

            // ICH - Insert Characters: CSI n @
            b'@' => {
                let count = self.param_or_1(0) as usize;
                op.insert_chars(count);
            }

            // ECH - Erase Characters: CSI n X
            b'X' => {
                let count = self.param_or_1(0) as usize;
                op.erase_chars(count);
            }

            // DSR - Device Status Report: CSI n n
            b'n' => {
                if self.param_or(0, 0) == 6 {
                    op.report_cursor_position();
                }
            }

            // SU - Scroll Up: CSI n S
            b'S' => {
                let count = self.param_or_1(0) as usize;
                op.scroll_up(count);
            }

            // SD - Scroll Down: CSI n T
            b'T' => {
                let count = self.param_or_1(0) as usize;
                op.scroll_down(count);
            }

            // DECSCUSR - Set Cursor Style: CSI Ps SP q or CSI Ps q
            b'q' => {
                let ps = self.param_or(0, 0);
                let style = match ps {
                    0 => CursorStyle::Default,
                    1 => CursorStyle::BlinkingBlock,
                    2 => CursorStyle::SteadyBlock,
                    3 => CursorStyle::BlinkingUnderline,
                    4 => CursorStyle::SteadyUnderline,
                    5 => CursorStyle::BlinkingBar,
                    6 => CursorStyle::SteadyBar,
                    _ => CursorStyle::Default,
                };
                op.set_cursor_style(style);
            }

            _ => {}
        }
    }

    fn handle_sgr<T: EscapeOp>(&self, num_params: usize, op: &mut T) {
        if num_params == 0 {
            op.set_fg_color(Pixel::WHITE);
            op.set_bg_color(Pixel::BLACK);
            op.set_inverse(false);
            return;
        }

        let mut cursor = 0;
        while cursor < num_params {
            let op_code = self.param_or(cursor, 0) as u8;
            cursor += 1;

            match op_code {
                0 => {
                    op.set_fg_color(Pixel::WHITE);
                    op.set_bg_color(Pixel::BLACK);
                    op.set_inverse(false);
                }
                7 => op.set_inverse(true),
                27 => op.set_inverse(false),
                30..=37 => op.set_fg_color(COLORS[op_code as usize - 30]),
                38 if num_params - cursor >= 2 && self.param_or(cursor, 0) == 5 => {
                    op.set_fg_color(Self::get_256_color(self.param_or(cursor + 1, 0) as u8));
                    cursor += 2;
                }
                38 if num_params - cursor >= 4 && self.param_or(cursor, 0) == 2 => {
                    op.set_fg_color(Pixel {
                        red: self.param_or(cursor + 1, 0) as u8,
                        green: self.param_or(cursor + 2, 0) as u8,
                        blue: self.param_or(cursor + 3, 0) as u8,
                    });
                    cursor += 4;
                }
                39 => op.set_fg_color(Pixel::WHITE),
                90..=97 => op.set_fg_color(COLORS[op_code as usize - 90 + 8]),

                40..=47 => op.set_bg_color(COLORS[op_code as usize - 40]),
                48 if num_params - cursor >= 2 && self.param_or(cursor, 0) == 5 => {
                    op.set_bg_color(Self::get_256_color(self.param_or(cursor + 1, 0) as u8));
                    cursor += 2;
                }
                48 if num_params - cursor >= 4 && self.param_or(cursor, 0) == 2 => {
                    op.set_bg_color(Pixel {
                        red: self.param_or(cursor + 1, 0) as u8,
                        green: self.param_or(cursor + 2, 0) as u8,
                        blue: self.param_or(cursor + 3, 0) as u8,
                    });
                    cursor += 4;
                }
                49 => op.set_bg_color(Pixel::BLACK),
                100..=107 => op.set_bg_color(COLORS[op_code as usize - 100 + 8]),

                _ => {}
            }
        }
    }

    fn get_256_color(i: u8) -> Pixel {
        let mut i = i as u16;
        let (red, green, blue) = match i {
            0..=7 => {
                let r = if i & 1 != 0 { 0xAA } else { 0x00 };
                let g = if i & 2 != 0 { 0xAA } else { 0x00 };
                let b = if i & 4 != 0 { 0xAA } else { 0x00 };
                (r, g, b)
            }
            8..=15 => {
                let r = if i & 1 != 0 { 0xFF } else { 0x55 };
                let g = if i & 2 != 0 { 0xFF } else { 0x55 };
                let b = if i & 4 != 0 { 0xFF } else { 0x55 };
                (r, g, b)
            }
            16..=231 => {
                i -= 16;
                let b = i % 6 * 255 / 6;
                i /= 6;
                let g = i % 6 * 255 / 6;
                i /= 6;
                let r = i * 255 / 6;
                (r as u8, g as u8, b as u8)
            }
            _ => {
                let g = (i * 10 - 2312) as u8;
                (g, g, g)
            }
        };

        Pixel { red, green, blue }
    }

    fn param_or(&self, i: usize, default: u32) -> u32 {
        self.params.get(i).and_then(|p| *p).unwrap_or(default)
    }

    fn param_or_1(&self, i: usize) -> u32 {
        match self.params.get(i).and_then(|p| *p) {
            None | Some(0) => 1,
            Some(v) => v,
        }
    }
}

impl Default for EscapeFsm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use ostd::prelude::ktest;

    #[derive(Debug, PartialEq, Eq)]
    enum TestOp {
        SetCursor(usize, usize),
        MoveCursor(isize, isize),
        SetCursorCol(usize),
        SetCursorRow(usize),
        SaveCursor,
        RestoreCursor,
        SetCursorVisible(bool),
        SetInverse(bool),
        Reset,
        SetFg(Pixel),
        SetBg(Pixel),
        ClearDisplay(EraseInDisplay),
        ClearLine(EraseInLine),
        SetCursorKeyMode(bool),
        SetAutowrap(bool),
    }

    struct MockOp {
        ops: Vec<TestOp>,
    }

    impl MockOp {
        fn new() -> Self {
            Self { ops: Vec::new() }
        }
    }

    impl EscapeOp for MockOp {
        fn set_cursor(&mut self, x: usize, y: usize) {
            self.ops.push(TestOp::SetCursor(x, y));
        }
        fn move_cursor(&mut self, dx: isize, dy: isize) {
            self.ops.push(TestOp::MoveCursor(dx, dy));
        }
        fn set_cursor_col(&mut self, col: usize) {
            self.ops.push(TestOp::SetCursorCol(col));
        }
        fn set_cursor_row(&mut self, row: usize) {
            self.ops.push(TestOp::SetCursorRow(row));
        }
        fn save_cursor(&mut self) {
            self.ops.push(TestOp::SaveCursor);
        }
        fn restore_cursor(&mut self) {
            self.ops.push(TestOp::RestoreCursor);
        }
        fn set_cursor_visible(&mut self, visible: bool) {
            self.ops.push(TestOp::SetCursorVisible(visible));
        }
        fn set_cursor_key_mode(&mut self, mode: bool) {
            self.ops.push(TestOp::SetCursorKeyMode(mode));
        }
        fn set_autowrap(&mut self, mode: bool) {
            self.ops.push(TestOp::SetAutowrap(mode));
        }
        fn set_inverse(&mut self, val: bool) {
            self.ops.push(TestOp::SetInverse(val));
        }
        fn reset(&mut self) {
            self.ops.push(TestOp::Reset);
        }
        fn set_fg_color(&mut self, val: Pixel) {
            self.ops.push(TestOp::SetFg(val));
        }
        fn set_bg_color(&mut self, val: Pixel) {
            self.ops.push(TestOp::SetBg(val));
        }
        fn erase_in_display(&mut self, mode: EraseInDisplay) {
            self.ops.push(TestOp::ClearDisplay(mode));
        }
        fn erase_in_line(&mut self, mode: EraseInLine) {
            self.ops.push(TestOp::ClearLine(mode));
        }
    }

    #[ktest]
    fn test_apk_progress_bar_escapes() {
        let mut fsm = EscapeFsm::new();
        let mut mock = MockOp::new();

        // apk progress bar sequence: \e7 (save), text, \e8 (restore), \e[0K (clear line)
        for &b in b"\x1b7progress\x1b8\x1b[0K" {
            fsm.eat(b, &mut mock);
        }

        assert_eq!(mock.ops, alloc::vec![
            TestOp::SaveCursor,
            TestOp::RestoreCursor,
            TestOp::ClearLine(EraseInLine::CursorToEnd),
        ]);
    }

    #[ktest]
    fn test_cursor_visibility_and_sco() {
        let mut fsm = EscapeFsm::new();
        let mut mock = MockOp::new();

        // DECTCEM hide cursor \e[?25l and show cursor \e[?25h
        for &b in b"\x1b[?25l\x1b[?25h" {
            fsm.eat(b, &mut mock);
        }
        assert_eq!(mock.ops, alloc::vec![
            TestOp::SetCursorVisible(false),
            TestOp::SetCursorVisible(true),
        ]);

        // SCO save \e[s and restore \e[u
        mock.ops.clear();
        for &b in b"\x1b[s\x1b[u" {
            fsm.eat(b, &mut mock);
        }
        assert_eq!(mock.ops, alloc::vec![
            TestOp::SaveCursor,
            TestOp::RestoreCursor,
        ]);

        // CHA \e[15G and VPA \e[8d
        mock.ops.clear();
        for &b in b"\x1b[15G\x1b[8d" {
            fsm.eat(b, &mut mock);
        }
        assert_eq!(mock.ops, alloc::vec![
            TestOp::SetCursorCol(14),
            TestOp::SetCursorRow(7),
        ]);
    }

    #[ktest]
    fn test_decckm_and_decawm() {
        let mut fsm = EscapeFsm::new();
        let mut mock = MockOp::new();

        // DECCKM \e[?1h and \e[?1l, DECAWM \e[?7l and \e[?7h
        for &b in b"\x1b[?1h\x1b[?7l\x1b[?1l\x1b[?7h" {
            fsm.eat(b, &mut mock);
        }
        assert_eq!(mock.ops, alloc::vec![
            TestOp::SetCursorKeyMode(true),
            TestOp::SetAutowrap(false),
            TestOp::SetCursorKeyMode(false),
            TestOp::SetAutowrap(true),
        ]);

        // CUP with 0 defaults: \e[0;0H -> (0, 0), \e[H -> (0, 0)
        mock.ops.clear();
        for &b in b"\x1b[0;0H\x1b[H" {
            fsm.eat(b, &mut mock);
        }
        assert_eq!(mock.ops, alloc::vec![
            TestOp::SetCursor(0, 0),
            TestOp::SetCursor(0, 0),
        ]);
    }
}
