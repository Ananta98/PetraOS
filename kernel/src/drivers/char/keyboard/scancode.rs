//! PS/2 Keyboard Scancode Set 1 Decoder and Key Mapping
//!
//! Translates hardware scancodes (Scancode Set 1) into key events, KeyCodes,
//! and ASCII characters based on the standard US QWERTY keyboard layout.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Escape,
    Backspace,
    Tab,
    Enter,
    LeftCtrl,
    RightCtrl,
    LeftShift,
    RightShift,
    LeftAlt,
    RightAlt,
    CapsLock,
    NumLock,
    ScrollLock,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Unknown(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub state: KeyState,
    pub ascii: Option<char>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
    pub scroll_lock: bool,
}

pub struct ScancodeDecoder {
    extended: bool,
    modifiers: Modifiers,
}

impl ScancodeDecoder {
    pub const fn new() -> Self {
        Self {
            extended: false,
            modifiers: Modifiers {
                shift: false,
                ctrl: false,
                alt: false,
                caps_lock: false,
                num_lock: false,
                scroll_lock: false,
            },
        }
    }

    pub fn modifiers(&self) -> &Modifiers {
        &self.modifiers
    }

    /// Process a single scancode byte and return a `KeyEvent` if a complete event is formed.
    pub fn process_scancode(&mut self, scancode: u8) -> Option<KeyEvent> {
        if scancode == 0xE0 {
            self.extended = true;
            return None;
        }

        let is_extended = self.extended;
        self.extended = false;

        let is_release = (scancode & 0x80) != 0;
        let make_code = scancode & 0x7F;

        if is_extended {
            return self.process_extended(make_code, is_release);
        }

        self.process_standard(make_code, is_release)
    }

    fn process_extended(&mut self, make_code: u8, is_release: bool) -> Option<KeyEvent> {
        let state = if is_release {
            KeyState::Released
        } else {
            KeyState::Pressed
        };

        let (code, ascii) = match make_code {
            0x1C => (KeyCode::Enter, if !is_release { Some('\n') } else { None }), // Keypad enter
            0x1D => {
                self.modifiers.ctrl = !is_release;
                (KeyCode::RightCtrl, None)
            }
            0x38 => {
                self.modifiers.alt = !is_release;
                (KeyCode::RightAlt, None)
            }
            0x48 => (KeyCode::Up, None),
            0x4B => (KeyCode::Left, None),
            0x4D => (KeyCode::Right, None),
            0x50 => (KeyCode::Down, None),
            0x47 => (KeyCode::Home, None),
            0x4F => (KeyCode::End, None),
            0x49 => (KeyCode::PageUp, None),
            0x51 => (KeyCode::PageDown, None),
            0x52 => (KeyCode::Insert, None),
            0x53 => (KeyCode::Delete, None),
            _ => (KeyCode::Unknown(make_code), None),
        };

        Some(KeyEvent { code, state, ascii })
    }

    fn process_standard(&mut self, make_code: u8, is_release: bool) -> Option<KeyEvent> {
        let state = if is_release {
            KeyState::Released
        } else {
            KeyState::Pressed
        };

        // Handle modifier keys
        match make_code {
            0x2A | 0x36 => {
                // Left Shift (0x2A), Right Shift (0x36)
                self.modifiers.shift = !is_release;
                let code = if make_code == 0x2A {
                    KeyCode::LeftShift
                } else {
                    KeyCode::RightShift
                };
                return Some(KeyEvent {
                    code,
                    state,
                    ascii: None,
                });
            }
            0x1D => {
                // Left Ctrl
                self.modifiers.ctrl = !is_release;
                return Some(KeyEvent {
                    code: KeyCode::LeftCtrl,
                    state,
                    ascii: None,
                });
            }
            0x38 => {
                // Left Alt
                self.modifiers.alt = !is_release;
                return Some(KeyEvent {
                    code: KeyCode::LeftAlt,
                    state,
                    ascii: None,
                });
            }
            0x3A => {
                // Caps Lock (toggle on press only)
                if !is_release {
                    self.modifiers.caps_lock = !self.modifiers.caps_lock;
                }
                return Some(KeyEvent {
                    code: KeyCode::CapsLock,
                    state,
                    ascii: None,
                });
            }
            0x45 => {
                // Num Lock (toggle on press only)
                if !is_release {
                    self.modifiers.num_lock = !self.modifiers.num_lock;
                }
                return Some(KeyEvent {
                    code: KeyCode::NumLock,
                    state,
                    ascii: None,
                });
            }
            0x46 => {
                // Scroll Lock (toggle on press only)
                if !is_release {
                    self.modifiers.scroll_lock = !self.modifiers.scroll_lock;
                }
                return Some(KeyEvent {
                    code: KeyCode::ScrollLock,
                    state,
                    ascii: None,
                });
            }
            _ => {}
        }

        let code = self.map_keycode(make_code);
        let ascii = if !is_release {
            self.map_ascii(make_code)
        } else {
            None
        };

        Some(KeyEvent { code, state, ascii })
    }

    fn map_keycode(&self, make_code: u8) -> KeyCode {
        match make_code {
            0x01 => KeyCode::Escape,
            0x0E => KeyCode::Backspace,
            0x0F => KeyCode::Tab,
            0x1C => KeyCode::Enter,
            0x3B => KeyCode::F1,
            0x3C => KeyCode::F2,
            0x3D => KeyCode::F3,
            0x3E => KeyCode::F4,
            0x3F => KeyCode::F5,
            0x40 => KeyCode::F6,
            0x41 => KeyCode::F7,
            0x42 => KeyCode::F8,
            0x43 => KeyCode::F9,
            0x44 => KeyCode::F10,
            0x57 => KeyCode::F11,
            0x58 => KeyCode::F12,
            0x47 => KeyCode::Home,
            0x48 => KeyCode::Up,
            0x49 => KeyCode::PageUp,
            0x4B => KeyCode::Left,
            0x4D => KeyCode::Right,
            0x4F => KeyCode::End,
            0x50 => KeyCode::Down,
            0x51 => KeyCode::PageDown,
            0x52 => KeyCode::Insert,
            0x53 => KeyCode::Delete,
            _ => {
                if let Some(ch) = self.map_ascii(make_code) {
                    KeyCode::Char(ch)
                } else {
                    KeyCode::Unknown(make_code)
                }
            }
        }
    }

    fn map_ascii(&self, make_code: u8) -> Option<char> {
        let shift = self.modifiers.shift;
        let caps = self.modifiers.caps_lock;
        let effective_shift_for_alpha = shift ^ caps;

        let ch = match make_code {
            0x01 => '\x1b', // ESC
            0x02 => {
                if shift {
                    '!'
                } else {
                    '1'
                }
            }
            0x03 => {
                if shift {
                    '@'
                } else {
                    '2'
                }
            }
            0x04 => {
                if shift {
                    '#'
                } else {
                    '3'
                }
            }
            0x05 => {
                if shift {
                    '$'
                } else {
                    '4'
                }
            }
            0x06 => {
                if shift {
                    '%'
                } else {
                    '5'
                }
            }
            0x07 => {
                if shift {
                    '^'
                } else {
                    '6'
                }
            }
            0x08 => {
                if shift {
                    '&'
                } else {
                    '7'
                }
            }
            0x09 => {
                if shift {
                    '*'
                } else {
                    '8'
                }
            }
            0x0A => {
                if shift {
                    '('
                } else {
                    '9'
                }
            }
            0x0B => {
                if shift {
                    ')'
                } else {
                    '0'
                }
            }
            0x0C => {
                if shift {
                    '_'
                } else {
                    '-'
                }
            }
            0x0D => {
                if shift {
                    '+'
                } else {
                    '='
                }
            }
            0x0E => '\x08', // Backspace
            0x0F => '\t',   // Tab
            0x10 => {
                if effective_shift_for_alpha {
                    'Q'
                } else {
                    'q'
                }
            }
            0x11 => {
                if effective_shift_for_alpha {
                    'W'
                } else {
                    'w'
                }
            }
            0x12 => {
                if effective_shift_for_alpha {
                    'E'
                } else {
                    'e'
                }
            }
            0x13 => {
                if effective_shift_for_alpha {
                    'R'
                } else {
                    'r'
                }
            }
            0x14 => {
                if effective_shift_for_alpha {
                    'T'
                } else {
                    't'
                }
            }
            0x15 => {
                if effective_shift_for_alpha {
                    'Y'
                } else {
                    'y'
                }
            }
            0x16 => {
                if effective_shift_for_alpha {
                    'U'
                } else {
                    'u'
                }
            }
            0x17 => {
                if effective_shift_for_alpha {
                    'I'
                } else {
                    'i'
                }
            }
            0x18 => {
                if effective_shift_for_alpha {
                    'O'
                } else {
                    'o'
                }
            }
            0x19 => {
                if effective_shift_for_alpha {
                    'P'
                } else {
                    'p'
                }
            }
            0x1A => {
                if shift {
                    '{'
                } else {
                    '['
                }
            }
            0x1B => {
                if shift {
                    '}'
                } else {
                    ']'
                }
            }
            0x1C => '\n', // Enter
            0x1E => {
                if effective_shift_for_alpha {
                    'A'
                } else {
                    'a'
                }
            }
            0x1F => {
                if effective_shift_for_alpha {
                    'S'
                } else {
                    's'
                }
            }
            0x20 => {
                if effective_shift_for_alpha {
                    'D'
                } else {
                    'd'
                }
            }
            0x21 => {
                if effective_shift_for_alpha {
                    'F'
                } else {
                    'f'
                }
            }
            0x22 => {
                if effective_shift_for_alpha {
                    'G'
                } else {
                    'g'
                }
            }
            0x23 => {
                if effective_shift_for_alpha {
                    'H'
                } else {
                    'h'
                }
            }
            0x24 => {
                if effective_shift_for_alpha {
                    'J'
                } else {
                    'j'
                }
            }
            0x25 => {
                if effective_shift_for_alpha {
                    'K'
                } else {
                    'k'
                }
            }
            0x26 => {
                if effective_shift_for_alpha {
                    'L'
                } else {
                    'l'
                }
            }
            0x27 => {
                if shift {
                    ':'
                } else {
                    ';'
                }
            }
            0x28 => {
                if shift {
                    '"'
                } else {
                    '\''
                }
            }
            0x29 => {
                if shift {
                    '~'
                } else {
                    '`'
                }
            }
            0x2B => {
                if shift {
                    '|'
                } else {
                    '\\'
                }
            }
            0x2C => {
                if effective_shift_for_alpha {
                    'Z'
                } else {
                    'z'
                }
            }
            0x2D => {
                if effective_shift_for_alpha {
                    'X'
                } else {
                    'x'
                }
            }
            0x2E => {
                if effective_shift_for_alpha {
                    'C'
                } else {
                    'c'
                }
            }
            0x2F => {
                if effective_shift_for_alpha {
                    'V'
                } else {
                    'v'
                }
            }
            0x30 => {
                if effective_shift_for_alpha {
                    'B'
                } else {
                    'b'
                }
            }
            0x31 => {
                if effective_shift_for_alpha {
                    'N'
                } else {
                    'n'
                }
            }
            0x32 => {
                if effective_shift_for_alpha {
                    'M'
                } else {
                    'm'
                }
            }
            0x33 => {
                if shift {
                    '<'
                } else {
                    ','
                }
            }
            0x34 => {
                if shift {
                    '>'
                } else {
                    '.'
                }
            }
            0x35 => {
                if shift {
                    '?'
                } else {
                    '/'
                }
            }
            0x37 => '*', // Keypad *
            0x39 => ' ', // Space
            0x4A => '-', // Keypad -
            0x4E => '+', // Keypad +
            0x47 => {
                if self.modifiers.num_lock {
                    '7'
                } else {
                    return None;
                }
            }
            0x48 => {
                if self.modifiers.num_lock {
                    '8'
                } else {
                    return None;
                }
            }
            0x49 => {
                if self.modifiers.num_lock {
                    '9'
                } else {
                    return None;
                }
            }
            0x4B => {
                if self.modifiers.num_lock {
                    '4'
                } else {
                    return None;
                }
            }
            0x4C => {
                if self.modifiers.num_lock {
                    '5'
                } else {
                    return None;
                }
            }
            0x4D => {
                if self.modifiers.num_lock {
                    '6'
                } else {
                    return None;
                }
            }
            0x4F => {
                if self.modifiers.num_lock {
                    '1'
                } else {
                    return None;
                }
            }
            0x50 => {
                if self.modifiers.num_lock {
                    '2'
                } else {
                    return None;
                }
            }
            0x51 => {
                if self.modifiers.num_lock {
                    '3'
                } else {
                    return None;
                }
            }
            0x52 => {
                if self.modifiers.num_lock {
                    '0'
                } else {
                    return None;
                }
            }
            0x53 => {
                if self.modifiers.num_lock {
                    '.'
                } else {
                    return None;
                }
            }
            _ => return None,
        };

        if self.modifiers.ctrl {
            if ch.is_ascii_alphabetic() {
                let ctrl_code = (ch.to_ascii_uppercase() as u8 - b'A' + 1) as char;
                return Some(ctrl_code);
            }
            if ch == '[' {
                return Some('\x1b');
            }
            if ch == '\\' {
                return Some('\x1c');
            }
            if ch == ']' {
                return Some('\x1d');
            }
            if ch == '^' {
                return Some('\x1e');
            }
            if ch == '_' {
                return Some('\x1f');
            }
        }

        Some(ch)
    }
}
