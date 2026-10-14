// SPDX-License-Identifier: GPL-2.0

//! ANSI / VT100 Terminal Escape Sequence Parser.
//!
//! Implements a resilient, zero-allocation terminal state machine inspired
//! by FreeBSD's `teken` and Linux's `vt.c` for parsing VT100/ANSI CSI escape
//! sequences, SGR color codes, cursor motion, and screen clearing commands.

use super::pixel::Color;

/// Parsed ANSI terminal control action to execute on the console surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnsiAction {
    /// Render a single printable ASCII character.
    Print(char),
    /// Line Feed (`\n`): Move cursor down, scrolling if at bottom margin.
    Newline,
    /// Carriage Return (`\r`): Move cursor to column 0.
    CarriageReturn,
    /// Tab (`\t`): Advance to next 8-column tab stop.
    Tab,
    /// Backspace (`\x08`): Move cursor left 1 column (non-destructive).
    Backspace,
    /// Delete characters at cursor (DCH - CSI P).
    DeleteChars(usize),
    /// Erase characters at cursor (ECH - CSI X).
    EraseChars(usize),
    /// Select Graphic Rendition (SGR) text styling.
    SetStyle {
        fg: Option<Color>,
        bg: Option<Color>,
        bold: bool,
        inverse: bool,
        reset: bool,
    },
    /// Move cursor to absolute 0-indexed grid coordinates `(col, row)`.
    SetCursorPos { col: usize, row: usize },
    /// Move cursor relative to current position.
    MoveCursor { dcol: isize, drow: isize },
    /// Move cursor to absolute 0-indexed column in the current row (CHA - CSI G / CSI ').
    SetCursorCol(usize),
    /// Move cursor to absolute 0-indexed row in the current column (VPA - CSI d).
    SetCursorRow(usize),
    /// Save current cursor position and text style (DECSC - ESC 7 / ANSI.SYS CSI s).
    SaveCursor,
    /// Restore previously saved cursor position and text style (DECRC - ESC 8 / ANSI.SYS CSI u).
    RestoreCursor,
    /// Clear screen: 0 = cursor to end, 1 = start to cursor, 2 = full screen.
    ClearDisplay(u8),
    /// Clear line: 0 = cursor to end, 1 = start to cursor, 2 = full line.
    ClearLine(u8),
    /// Set hardware cursor visibility (DEC private mode 25).
    SetCursorVisible(bool),
    /// Reset terminal to default initial state (RIS).
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    Csi,
}

/// ANSI / VT100 state machine parser.
pub struct AnsiParser {
    state: State,
    params: [u16; 8],
    param_count: usize,
    in_param: bool,
    is_private: bool,
}

impl AnsiParser {
    pub const fn new() -> Self {
        Self {
            state: State::Ground,
            params: [0; 8],
            param_count: 0,
            in_param: false,
            is_private: false,
        }
    }

    /// Process a single incoming byte and execute corresponding actions via callback.
    pub fn process_byte<F>(&mut self, byte: u8, mut emit: F)
    where
        F: FnMut(AnsiAction),
    {
        match self.state {
            State::Ground => match byte {
                0x1B => {
                    self.state = State::Escape;
                }
                b'\n' => emit(AnsiAction::Newline),
                b'\r' => emit(AnsiAction::CarriageReturn),
                b'\t' => emit(AnsiAction::Tab),
                0x08 => emit(AnsiAction::Backspace),
                0x7F => { /* DEL - ignored in VT100 ground state */ }
                0x07 => { /* Bell - ignore or flash */ }
                ch if ch >= 0x20 => {
                    emit(AnsiAction::Print(ch as char));
                }
                _ => {}
            },

            State::Escape => match byte {
                b'[' => {
                    self.state = State::Csi;
                    self.params = [0; 8];
                    self.param_count = 0;
                    self.in_param = false;
                    self.is_private = false;
                }
                b'7' => {
                    // DECSC - DEC Save Cursor
                    emit(AnsiAction::SaveCursor);
                    self.state = State::Ground;
                }
                b'8' => {
                    // DECRC - DEC Restore Cursor
                    emit(AnsiAction::RestoreCursor);
                    self.state = State::Ground;
                }
                b'M' => {
                    // RI - Reverse Index (cursor up)
                    emit(AnsiAction::MoveCursor { dcol: 0, drow: -1 });
                    self.state = State::Ground;
                }
                b'E' => {
                    // NEL - Next Line
                    emit(AnsiAction::CarriageReturn);
                    emit(AnsiAction::Newline);
                    self.state = State::Ground;
                }
                b'c' => {
                    // RIS - Reset Initial State
                    emit(AnsiAction::Reset);
                    self.state = State::Ground;
                }
                _ => {
                    // Unknown escape sequence, abort back to Ground
                    self.state = State::Ground;
                }
            },

            State::Csi => match byte {
                b'?' => {
                    self.is_private = true;
                }
                b'0'..=b'9' => {
                    self.in_param = true;
                    if self.param_count < self.params.len() {
                        let val = (byte - b'0') as u16;
                        self.params[self.param_count] = self.params[self.param_count]
                            .saturating_mul(10)
                            .saturating_add(val);
                    }
                }
                b';' => {
                    if self.in_param && self.param_count < self.params.len() {
                        self.param_count += 1;
                    }
                    self.in_param = false;
                }
                // Terminators
                b'm' => {
                    if self.in_param && self.param_count < self.params.len() {
                        self.param_count += 1;
                    }
                    self.dispatch_sgr(&mut emit);
                    self.state = State::Ground;
                }
                b'H' | b'f' => {
                    if self.in_param && self.param_count < self.params.len() {
                        self.param_count += 1;
                    }
                    // 1-indexed coordinates: default row=1, col=1
                    let row = if self.param_count > 0 && self.params[0] > 0 {
                        (self.params[0] - 1) as usize
                    } else {
                        0
                    };
                    let col = if self.param_count > 1 && self.params[1] > 0 {
                        (self.params[1] - 1) as usize
                    } else {
                        0
                    };
                    emit(AnsiAction::SetCursorPos { col, row });
                    self.state = State::Ground;
                }
                b'A' => {
                    // Cursor Up
                    let count = if self.in_param && self.params[0] > 0 { self.params[0] as isize } else { 1 };
                    emit(AnsiAction::MoveCursor { dcol: 0, drow: -count });
                    self.state = State::Ground;
                }
                b'B' => {
                    // Cursor Down
                    let count = if self.in_param && self.params[0] > 0 { self.params[0] as isize } else { 1 };
                    emit(AnsiAction::MoveCursor { dcol: 0, drow: count });
                    self.state = State::Ground;
                }
                b'C' => {
                    // Cursor Forward
                    let count = if self.in_param && self.params[0] > 0 { self.params[0] as isize } else { 1 };
                    emit(AnsiAction::MoveCursor { dcol: count, drow: 0 });
                    self.state = State::Ground;
                }
                b'D' => {
                    // Cursor Back
                    let count = if self.in_param && self.params[0] > 0 { self.params[0] as isize } else { 1 };
                    emit(AnsiAction::MoveCursor { dcol: -count, drow: 0 });
                    self.state = State::Ground;
                }
                b'J' => {
                    // Erase in Display
                    let mode = if self.in_param { self.params[0] as u8 } else { 0 };
                    emit(AnsiAction::ClearDisplay(mode));
                    self.state = State::Ground;
                }
                b'K' => {
                    // Erase in Line
                    let mode = if self.in_param { self.params[0] as u8 } else { 0 };
                    emit(AnsiAction::ClearLine(mode));
                    self.state = State::Ground;
                }
                b'P' => {
                    // DCH - Delete Character (shifts line left)
                    let count = if self.in_param && self.params[0] > 0 { self.params[0] as usize } else { 1 };
                    emit(AnsiAction::DeleteChars(count));
                    self.state = State::Ground;
                }
                b'X' => {
                    // ECH - Erase Character (erases to space in place)
                    let count = if self.in_param && self.params[0] > 0 { self.params[0] as usize } else { 1 };
                    emit(AnsiAction::EraseChars(count));
                    self.state = State::Ground;
                }
                b'h' => {
                    if self.is_private && self.params[0] == 25 {
                        emit(AnsiAction::SetCursorVisible(true));
                    }
                    self.state = State::Ground;
                }
                b'l' => {
                    if self.is_private && self.params[0] == 25 {
                        emit(AnsiAction::SetCursorVisible(false));
                    }
                    self.state = State::Ground;
                }
                b's' => {
                    // SCO / ANSI save cursor: CSI s
                    emit(AnsiAction::SaveCursor);
                    self.state = State::Ground;
                }
                b'u' => {
                    // SCO / ANSI restore cursor: CSI u
                    emit(AnsiAction::RestoreCursor);
                    self.state = State::Ground;
                }
                b'G' | b'\'' => {
                    // CHA - Cursor Character Absolute (1-indexed column)
                    let col = if self.in_param && self.params[0] > 0 {
                        (self.params[0] - 1) as usize
                    } else {
                        0
                    };
                    emit(AnsiAction::SetCursorCol(col));
                    self.state = State::Ground;
                }
                b'd' => {
                    // VPA - Line Position Absolute (1-indexed row)
                    let row = if self.in_param && self.params[0] > 0 {
                        (self.params[0] - 1) as usize
                    } else {
                        0
                    };
                    emit(AnsiAction::SetCursorRow(row));
                    self.state = State::Ground;
                }
                _ => {
                    // Unrecognized sequence terminator
                    self.state = State::Ground;
                }
            },
        }
    }

    fn dispatch_sgr<F>(&self, emit: &mut F)
    where
        F: FnMut(AnsiAction),
    {
        if self.param_count == 0 {
            // Default `\x1b[m` resets all attributes
            emit(AnsiAction::SetStyle {
                fg: None,
                bg: None,
                bold: false,
                inverse: false,
                reset: true,
            });
            return;
        }

        let mut fg = None;
        let mut bg = None;
        let mut bold = false;
        let mut inverse = false;
        let mut reset = false;

        for &p in &self.params[..self.param_count] {
            match p {
                0 => reset = true,
                1 => bold = true,
                7 => inverse = true,
                30..=37 => fg = Some(Color::from_ansi((p - 30) as u8)),
                39 => fg = None, // Reset fg to default
                40..=47 => bg = Some(Color::from_ansi((p - 40) as u8)),
                49 => bg = None, // Reset bg to default
                90..=97 => fg = Some(Color::from_ansi((p - 90 + 8) as u8)),
                100..=107 => bg = Some(Color::from_ansi((p - 100 + 8) as u8)),
                _ => {}
            }
        }

        emit(AnsiAction::SetStyle {
            fg,
            bg,
            bold,
            inverse,
            reset,
        });
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_ansi_dec_and_sco_cursor_save_restore() {
        let mut parser = AnsiParser::new();
        let mut actions = Vec::new();

        // 1. Test DECSC "\x1b7" and DECRC "\x1b8" (used by apk progress bar)
        for &b in b"\x1b7progress\x1b8\x1b[0K" {
            parser.process_byte(b, |a| actions.push(a));
        }

        assert_eq!(actions[0], AnsiAction::SaveCursor);
        assert_eq!(actions[1], AnsiAction::Print('p'));
        assert_eq!(actions[9], AnsiAction::RestoreCursor);
        assert_eq!(actions[10], AnsiAction::ClearLine(0));

        // 2. Test SCO CSI s and CSI u
        actions.clear();
        for &b in b"\x1b[s\x1b[u" {
            parser.process_byte(b, |a| actions.push(a));
        }
        assert_eq!(actions, alloc::vec![AnsiAction::SaveCursor, AnsiAction::RestoreCursor]);

        // 3. Test CHA (CSI G) and VPA (CSI d)
        actions.clear();
        for &b in b"\x1b[15G\x1b[8d" {
            parser.process_byte(b, |a| actions.push(a));
        }
        assert_eq!(actions, alloc::vec![AnsiAction::SetCursorCol(14), AnsiAction::SetCursorRow(7)]);
    }
}
