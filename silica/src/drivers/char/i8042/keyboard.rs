// SPDX-License-Identifier: GPL-2.0

//! i8042 PS/2 Keyboard Driver.
//!
//! Provides scancode decoding for Scan Code Sets 1 and 2 matching Asterinas specifications,
//! keycode mapping, modifier tracking (Shift, Ctrl, Alt, CapsLock), ANSI escape sequence
//! generation for navigation keys, and character device registration for `/dev/keyboard`.

use alloc::{string::String, sync::Arc};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use ostd::{
    arch::{
        irq::{IRQ_CHIP, MappedIrqLine},
        trap::TrapFrame,
    },
    irq::IrqLine,
};
use spin::{Mutex, Once};

use super::{
    controller::{I8042_CONTROLLER, I8042Controller},
    ps2::{CMD_ENABLE_SCANNING, CMD_RESET, send_keyboard_cmd},
};
use crate::{
    api::errno::Result, drivers::CharacterDevice, drivers::char::tty,
    utils::ring_buffer::RingBuffer,
};

const ISA_KEYBOARD_IRQ: u8 = 1;

/// IRQ line for i8042 keyboard.
static KEYBOARD_IRQ_LINE: Once<MappedIrqLine> = Once::new();

/// Global lock-free ring buffer storing received keyboard ASCII characters.
static KEYBOARD_BUFFER: RingBuffer<1024> = RingBuffer::new();

/// Global keyboard state machine for modifier tracking and character emission.
static KEYBOARD_STATE: Mutex<KeyboardState> = Mutex::new(KeyboardState::new());

/// Translation mode for keyboard scancodes.
static TRANSLATION_MODE: AtomicU8 = AtomicU8::new(TranslationMode::Hardware as u8);

// ----------------------------------------------------------------------------
// Key Codes & Status (Matching Asterinas Input Event Codes)
// ----------------------------------------------------------------------------

/// Standard input key codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    // Letters
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,

    // Digits
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Num0,

    // Whitespace and Control
    Space,
    Tab,
    Enter,
    Backspace,
    Esc,

    // Punctuation
    Minus,
    Equal,
    Grave,
    Backslash,
    Comma,
    Dot,
    Slash,
    Semicolon,
    Apostrophe,
    LeftBrace,
    RightBrace,

    // Modifier keys
    LeftCtrl,
    RightCtrl,
    LeftShift,
    RightShift,
    LeftAlt,
    RightAlt,
    CapsLock,

    // Function keys
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

    // Navigation and Editing keys
    Home,
    Up,
    PageUp,
    Left,
    Right,
    End,
    Down,
    PageDown,
    Insert,
    Delete,
}

/// The status of a key event (press or release).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStatus {
    Pressed,
    Released,
}

/// How Scan Code Set 2 scancodes are translated to Set 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TranslationMode {
    /// The i8042 controller translates Set 2 to Set 1 in hardware.
    Hardware = 0,
    /// No hardware translation; the driver translates Set 2 to Set 1 in software.
    Software = 1,
}

impl TranslationMode {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => Self::Hardware,
            _ => Self::Software,
        }
    }
}

// ----------------------------------------------------------------------------
// Scancode Decoding (Set 1 & Set 2 matching Asterinas)
// ----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct ScanCode(pub u8);

/// Scancode value and relevant modifier states.
#[derive(Clone, Debug)]
pub struct ScancodeInfo {
    pub scancode: ScanCode,
    pub key_status: KeyStatus,
    pub extended: bool,
}

impl ScancodeInfo {
    /// Reads the keyboard scancode from the i8042 controller, looping past prefixes.
    pub fn read() -> Option<Self> {
        let controller_lock = I8042_CONTROLLER.get()?;
        loop {
            let data = {
                let controller = controller_lock.lock();
                controller.receive_data()?
            };

            let mode = TranslationMode::from_u8(TRANSLATION_MODE.load(Ordering::Relaxed));
            let scancode = match mode {
                TranslationMode::Hardware => Self::read_set1(data),
                TranslationMode::Software => Self::read_set2(data),
            };

            if let Some(info) = scancode {
                return Some(info);
            }
        }
    }

    /// Interprets a byte as a Scan Code Set 1 scancode (hardware translation enabled).
    pub fn read_set1(data: u8) -> Option<Self> {
        static EXTENDED_KEY: AtomicBool = AtomicBool::new(false);

        let code = ScanCodeSet1(data);

        if code.has_error() {
            ostd::warn!("PS/2 keyboard: Key detection error or internal buffer overrun");
            return None;
        }

        // Handle the extension code.
        if code.is_extension() {
            EXTENDED_KEY.store(true, Ordering::Relaxed);
            return None;
        }

        let key_status = code.key_status();

        let extended = EXTENDED_KEY.load(Ordering::Relaxed);
        EXTENDED_KEY.store(false, Ordering::Relaxed);

        Some(Self {
            scancode: ScanCode(code.key()),
            key_status,
            extended,
        })
    }

    /// Interprets a byte as a Scan Code Set 2 scancode (no hardware translation).
    pub fn read_set2(data: u8) -> Option<Self> {
        static EXTENDED_KEY: AtomicBool = AtomicBool::new(false);
        static RELEASE_NEXT: AtomicBool = AtomicBool::new(false);

        let code = ScanCodeSet2(data);

        if code.has_error() {
            ostd::warn!("PS/2 keyboard: Key detection error or internal buffer overrun");
            return None;
        }

        // Handle the extension code.
        if code.is_extension() {
            EXTENDED_KEY.store(true, Ordering::Relaxed);
            return None;
        }

        // Handle the release prefix.
        if code.is_release_prefix() {
            RELEASE_NEXT.store(true, Ordering::Relaxed);
            return None;
        }

        let releasing = RELEASE_NEXT.load(Ordering::Relaxed);
        RELEASE_NEXT.store(false, Ordering::Relaxed);
        let extended = EXTENDED_KEY.load(Ordering::Relaxed);
        EXTENDED_KEY.store(false, Ordering::Relaxed);

        let set1_code = SET2_TO_SET1[code.raw() as usize];
        if set1_code == 0xFF {
            return None;
        }

        let key_status = if releasing {
            KeyStatus::Released
        } else {
            KeyStatus::Pressed
        };

        Some(Self {
            scancode: ScanCode(set1_code),
            key_status,
            extended,
        })
    }

    /// Maps the keyboard scancode to a [`KeyCode`] matching Asterinas scancode tables.
    pub fn to_key_code(&self) -> Option<KeyCode> {
        let ScanCode(code) = self.scancode;

        // Handle extended keys.
        if self.extended {
            return match code {
                0x47 => Some(KeyCode::Home),
                0x48 => Some(KeyCode::Up),
                0x49 => Some(KeyCode::PageUp),
                0x4B => Some(KeyCode::Left),
                0x4D => Some(KeyCode::Right),
                0x4F => Some(KeyCode::End),
                0x50 => Some(KeyCode::Down),
                0x51 => Some(KeyCode::PageDown),
                0x52 => Some(KeyCode::Insert),
                0x53 => Some(KeyCode::Delete),
                0x1D => Some(KeyCode::RightCtrl),
                0x38 => Some(KeyCode::RightAlt),
                _ => None,
            };
        }

        // Standard Set 1 key mapping.
        Some(match code {
            // Letters
            0x1E => KeyCode::A,
            0x30 => KeyCode::B,
            0x2E => KeyCode::C,
            0x20 => KeyCode::D,
            0x12 => KeyCode::E,
            0x21 => KeyCode::F,
            0x22 => KeyCode::G,
            0x23 => KeyCode::H,
            0x17 => KeyCode::I,
            0x24 => KeyCode::J,
            0x25 => KeyCode::K,
            0x26 => KeyCode::L,
            0x32 => KeyCode::M,
            0x31 => KeyCode::N,
            0x18 => KeyCode::O,
            0x19 => KeyCode::P,
            0x10 => KeyCode::Q,
            0x13 => KeyCode::R,
            0x1F => KeyCode::S,
            0x14 => KeyCode::T,
            0x16 => KeyCode::U,
            0x2F => KeyCode::V,
            0x11 => KeyCode::W,
            0x2D => KeyCode::X,
            0x15 => KeyCode::Y,
            0x2C => KeyCode::Z,

            // Digits
            0x02 => KeyCode::Num1,
            0x03 => KeyCode::Num2,
            0x04 => KeyCode::Num3,
            0x05 => KeyCode::Num4,
            0x06 => KeyCode::Num5,
            0x07 => KeyCode::Num6,
            0x08 => KeyCode::Num7,
            0x09 => KeyCode::Num8,
            0x0A => KeyCode::Num9,
            0x0B => KeyCode::Num0,

            // Whitespace and control
            0x39 => KeyCode::Space,
            0x0F => KeyCode::Tab,
            0x1C => KeyCode::Enter,
            0x0E => KeyCode::Backspace,

            // Punctuation
            0x0C => KeyCode::Minus,
            0x0D => KeyCode::Equal,
            0x29 => KeyCode::Grave,
            0x2B => KeyCode::Backslash,
            0x33 => KeyCode::Comma,
            0x34 => KeyCode::Dot,
            0x35 => KeyCode::Slash,
            0x27 => KeyCode::Semicolon,
            0x28 => KeyCode::Apostrophe,
            0x1A => KeyCode::LeftBrace,
            0x1B => KeyCode::RightBrace,

            // Modifier keys
            0x1D => KeyCode::LeftCtrl,
            0x2A => KeyCode::LeftShift,
            0x36 => KeyCode::RightShift,
            0x38 => KeyCode::LeftAlt,
            0x3A => KeyCode::CapsLock,

            // Function keys
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

            // Keypad navigation & editing (when NumLock is off or standard scancodes)
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

            // Escape
            0x01 => KeyCode::Esc,

            _ => return None,
        })
    }
}

/// A raw scan code byte in Scan Code Set 1.
#[derive(Clone, Copy, Debug)]
pub struct ScanCodeSet1(pub u8);

impl ScanCodeSet1 {
    pub const CODE_ERROR: u8 = 0xFF;
    pub const CODE_EXT_PREFIX: u8 = 0xE0;
    pub const RELEASE_MASK: u8 = 0x80;

    pub fn has_error(&self) -> bool {
        self.0 == Self::CODE_ERROR
    }

    pub fn key_status(&self) -> KeyStatus {
        if self.0 & Self::RELEASE_MASK == 0 {
            KeyStatus::Pressed
        } else {
            KeyStatus::Released
        }
    }

    pub fn is_extension(&self) -> bool {
        self.0 == Self::CODE_EXT_PREFIX
    }

    pub fn key(&self) -> u8 {
        self.0 & !Self::RELEASE_MASK
    }
}

/// A raw scan code byte in Scan Code Set 2.
#[derive(Clone, Copy, Debug)]
pub struct ScanCodeSet2(pub u8);

impl ScanCodeSet2 {
    pub const CODE_ERROR: u8 = 0xFF;
    pub const CODE_EXT_PREFIX: u8 = 0xE0;
    pub const CODE_RELEASE_PREFIX: u8 = 0xF0;

    pub fn has_error(&self) -> bool {
        self.0 == Self::CODE_ERROR
    }

    pub fn is_extension(&self) -> bool {
        self.0 == Self::CODE_EXT_PREFIX
    }

    pub fn is_release_prefix(&self) -> bool {
        self.0 == Self::CODE_RELEASE_PREFIX
    }

    pub fn raw(&self) -> u8 {
        self.0
    }
}

/// Scan Code Set 2 to Set 1 translation table.
pub const SET2_TO_SET1: [u8; 256] = [
    // 0x00-0x0F
    0xff, 0x43, 0x41, 0x3f, 0x3d, 0x3b, 0x3c, 0x58, 0x64, 0x44, 0x42, 0x40, 0x3e, 0x0f, 0x29, 0x59,
    // 0x10-0x1F
    0x65, 0x38, 0x2a, 0x70, 0x1d, 0x10, 0x02, 0x5a, 0x66, 0x71, 0x2c, 0x1f, 0x1e, 0x11, 0x03, 0x5b,
    // 0x20-0x2F
    0x67, 0x2e, 0x2d, 0x20, 0x12, 0x05, 0x04, 0x5c, 0x68, 0x39, 0x2f, 0x21, 0x14, 0x13, 0x06, 0x5d,
    // 0x30-0x3F
    0x69, 0x31, 0x30, 0x23, 0x22, 0x15, 0x07, 0x5e, 0x6a, 0x72, 0x32, 0x24, 0x16, 0x08, 0x09, 0x5f,
    // 0x40-0x4F
    0x6b, 0x33, 0x25, 0x17, 0x18, 0x0b, 0x0a, 0x60, 0x6c, 0x34, 0x35, 0x26, 0x27, 0x19, 0x0c, 0x61,
    // 0x50-0x5F
    0x6d, 0x73, 0x28, 0x74, 0x1a, 0x0d, 0x62, 0x6e, 0x3a, 0x36, 0x1c, 0x1b, 0x75, 0x2b, 0x63, 0x76,
    // 0x60-0x6F
    0x55, 0x56, 0x77, 0x78, 0x79, 0x7a, 0x0e, 0x7b, 0x7c, 0x4f, 0x7d, 0x4b, 0x47, 0x7e, 0x7f, 0x6f,
    // 0x70-0x7F
    0x52, 0x53, 0x50, 0x4c, 0x4d, 0x48, 0x01, 0x45, 0x57, 0x4e, 0x51, 0x4a, 0x37, 0x49, 0x46, 0x54,
    // 0x80-0x8F
    0x80, 0x81, 0x82, 0x41, 0x54, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f,
    // 0x90-0x9F
    0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0x9b, 0x9c, 0x9d, 0x9e, 0x9f,
    // 0xA0-0xAF
    0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf,
    // 0xB0-0xBF
    0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf,
    // 0xC0-0xCF
    0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf,
    // 0xD0-0xDF
    0xd0, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0xdd, 0xde, 0xdf,
    // 0xE0-0xEF
    0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xeb, 0xec, 0xed, 0xee, 0xef,
    // 0xF0-0xFF
    0xff, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, 0xfe, 0xff,
];

// ----------------------------------------------------------------------------
// Modifier Tracking & TTY Emission
// ----------------------------------------------------------------------------

/// Output action generated from a decoded key event.
#[derive(Debug, Clone, Copy)]
pub enum KeyAction {
    Char(char),
    Seq(&'static str),
}

/// Tracks keyboard modifier states and maps key events to terminal characters.
pub struct KeyboardState {
    lshift: bool,
    rshift: bool,
    lctrl: bool,
    rctrl: bool,
    lalt: bool,
    ralt: bool,
    caps_lock: bool,
}

impl KeyboardState {
    pub const fn new() -> Self {
        Self {
            lshift: false,
            rshift: false,
            lctrl: false,
            rctrl: false,
            lalt: false,
            ralt: false,
            caps_lock: false,
        }
    }

    /// Updates internal modifier status and returns printable characters or ANSI sequences.
    pub fn process_key(&mut self, key_code: KeyCode, key_status: KeyStatus) -> Option<KeyAction> {
        match key_status {
            KeyStatus::Pressed => self.handle_press(key_code),
            KeyStatus::Released => self.handle_release(key_code),
        }
    }

    fn handle_release(&mut self, key_code: KeyCode) -> Option<KeyAction> {
        match key_code {
            KeyCode::LeftShift => self.lshift = false,
            KeyCode::RightShift => self.rshift = false,
            KeyCode::LeftCtrl => self.lctrl = false,
            KeyCode::RightCtrl => self.rctrl = false,
            KeyCode::LeftAlt => self.lalt = false,
            KeyCode::RightAlt => self.ralt = false,
            _ => {}
        }
        None
    }

    fn handle_press(&mut self, key_code: KeyCode) -> Option<KeyAction> {
        // Track modifiers
        match key_code {
            KeyCode::LeftShift => {
                self.lshift = true;
                return None;
            }
            KeyCode::RightShift => {
                self.rshift = true;
                return None;
            }
            KeyCode::LeftCtrl => {
                self.lctrl = true;
                return None;
            }
            KeyCode::RightCtrl => {
                self.rctrl = true;
                return None;
            }
            KeyCode::LeftAlt => {
                self.lalt = true;
                return None;
            }
            KeyCode::RightAlt => {
                self.ralt = true;
                return None;
            }
            KeyCode::CapsLock => {
                self.caps_lock = !self.caps_lock;
                return None;
            }
            _ => {}
        }

        // Navigation and editing keys emit standard ANSI escape sequences
        let app_cursor = tty::is_application_cursor_keys();
        match key_code {
            KeyCode::Up => {
                return Some(KeyAction::Seq(if app_cursor { "\x1BOA" } else { "\x1B[A" }));
            }
            KeyCode::Down => {
                return Some(KeyAction::Seq(if app_cursor { "\x1BOB" } else { "\x1B[B" }));
            }
            KeyCode::Right => {
                return Some(KeyAction::Seq(if app_cursor { "\x1BOC" } else { "\x1B[C" }));
            }
            KeyCode::Left => {
                return Some(KeyAction::Seq(if app_cursor { "\x1BOD" } else { "\x1B[D" }));
            }
            KeyCode::Home => return Some(KeyAction::Seq("\x1B[1~")),
            KeyCode::End => return Some(KeyAction::Seq("\x1B[4~")),
            KeyCode::PageUp => return Some(KeyAction::Seq("\x1B[5~")),
            KeyCode::PageDown => return Some(KeyAction::Seq("\x1B[6~")),
            KeyCode::Insert => return Some(KeyAction::Seq("\x1B[2~")),
            KeyCode::Delete => return Some(KeyAction::Seq("\x1B[3~")),
            _ => {}
        }

        // Whitespace and control keys
        match key_code {
            KeyCode::Enter => return Some(KeyAction::Char('\r')),
            KeyCode::Backspace => return Some(KeyAction::Char('\x08')),
            KeyCode::Tab => return Some(KeyAction::Char('\t')),
            KeyCode::Esc => return Some(KeyAction::Char('\x1B')),
            KeyCode::Space => return Some(KeyAction::Char(' ')),
            _ => {}
        }

        let is_shifted = self.lshift || self.rshift;
        let is_ctrl = self.lctrl || self.rctrl;

        // Letters A-Z
        if let Some(letter_idx) = match key_code {
            KeyCode::A => Some(b'A'),
            KeyCode::B => Some(b'B'),
            KeyCode::C => Some(b'C'),
            KeyCode::D => Some(b'D'),
            KeyCode::E => Some(b'E'),
            KeyCode::F => Some(b'F'),
            KeyCode::G => Some(b'G'),
            KeyCode::H => Some(b'H'),
            KeyCode::I => Some(b'I'),
            KeyCode::J => Some(b'J'),
            KeyCode::K => Some(b'K'),
            KeyCode::L => Some(b'L'),
            KeyCode::M => Some(b'M'),
            KeyCode::N => Some(b'N'),
            KeyCode::O => Some(b'O'),
            KeyCode::P => Some(b'P'),
            KeyCode::Q => Some(b'Q'),
            KeyCode::R => Some(b'R'),
            KeyCode::S => Some(b'S'),
            KeyCode::T => Some(b'T'),
            KeyCode::U => Some(b'U'),
            KeyCode::V => Some(b'V'),
            KeyCode::W => Some(b'W'),
            KeyCode::X => Some(b'X'),
            KeyCode::Y => Some(b'Y'),
            KeyCode::Z => Some(b'Z'),
            _ => None,
        } {
            if is_ctrl {
                let ctrl_code = (letter_idx - b'A' + 1) as char;
                return Some(KeyAction::Char(ctrl_code));
            }
            let effective_shift = is_shifted ^ self.caps_lock;
            let ch = if effective_shift {
                letter_idx as char
            } else {
                (letter_idx + 32) as char
            };
            return Some(KeyAction::Char(ch));
        }

        // Digits 0-9
        let digit_char = match key_code {
            KeyCode::Num1 => {
                if is_shifted {
                    '!'
                } else {
                    '1'
                }
            }
            KeyCode::Num2 => {
                if is_shifted {
                    '@'
                } else {
                    '2'
                }
            }
            KeyCode::Num3 => {
                if is_shifted {
                    '#'
                } else {
                    '3'
                }
            }
            KeyCode::Num4 => {
                if is_shifted {
                    '$'
                } else {
                    '4'
                }
            }
            KeyCode::Num5 => {
                if is_shifted {
                    '%'
                } else {
                    '5'
                }
            }
            KeyCode::Num6 => {
                if is_shifted {
                    '^'
                } else {
                    '6'
                }
            }
            KeyCode::Num7 => {
                if is_shifted {
                    '&'
                } else {
                    '7'
                }
            }
            KeyCode::Num8 => {
                if is_shifted {
                    '*'
                } else {
                    '8'
                }
            }
            KeyCode::Num9 => {
                if is_shifted {
                    '('
                } else {
                    '9'
                }
            }
            KeyCode::Num0 => {
                if is_shifted {
                    ')'
                } else {
                    '0'
                }
            }

            // Punctuation
            KeyCode::Minus => {
                if is_shifted {
                    '_'
                } else {
                    '-'
                }
            }
            KeyCode::Equal => {
                if is_shifted {
                    '+'
                } else {
                    '='
                }
            }
            KeyCode::Grave => {
                if is_shifted {
                    '~'
                } else {
                    '`'
                }
            }
            KeyCode::Backslash => {
                if is_shifted {
                    '|'
                } else {
                    '\\'
                }
            }
            KeyCode::Comma => {
                if is_shifted {
                    '<'
                } else {
                    ','
                }
            }
            KeyCode::Dot => {
                if is_shifted {
                    '>'
                } else {
                    '.'
                }
            }
            KeyCode::Slash => {
                if is_shifted {
                    '?'
                } else {
                    '/'
                }
            }
            KeyCode::Semicolon => {
                if is_shifted {
                    ':'
                } else {
                    ';'
                }
            }
            KeyCode::Apostrophe => {
                if is_shifted {
                    '"'
                } else {
                    '\''
                }
            }
            KeyCode::LeftBrace => {
                if is_shifted {
                    '{'
                } else {
                    '['
                }
            }
            KeyCode::RightBrace => {
                if is_shifted {
                    '}'
                } else {
                    ']'
                }
            }
            _ => return None,
        };

        Some(KeyAction::Char(digit_char))
    }
}

// ----------------------------------------------------------------------------
// Interrupt Handler & Hardware Integration
// ----------------------------------------------------------------------------

fn handle_keyboard_irq(_frame: &TrapFrame) {
    let Some(controller_lock) = I8042_CONTROLLER.get() else {
        return;
    };

    loop {
        let maybe_data = {
            let controller = controller_lock.lock();
            controller.receive_data()
        };

        let Some(data) = maybe_data else {
            break;
        };

        let mode = TranslationMode::from_u8(TRANSLATION_MODE.load(Ordering::Relaxed));
        let maybe_scancode = match mode {
            TranslationMode::Hardware => ScancodeInfo::read_set1(data),
            TranslationMode::Software => ScancodeInfo::read_set2(data),
        };

        let Some(scancode_info) = maybe_scancode else {
            // Extension (0xE0) or release (0xF0) prefix consumed.
            // Spin briefly in case the controller is receiving the second byte across the PS/2 bus.
            for _ in 0..100 {
                let controller = controller_lock.lock();
                if controller.can_read() {
                    break;
                }
                drop(controller);
                core::hint::spin_loop();
            }
            continue;
        };

        let Some(key_code) = scancode_info.to_key_code() else {
            continue;
        };

        let maybe_action = {
            let mut state = KEYBOARD_STATE.lock();
            state.process_key(key_code, scancode_info.key_status)
        };

        match maybe_action {
            Some(KeyAction::Char(ch)) => {
                KEYBOARD_BUFFER.push_overwrite(ch as u8);
                tty::queue_input(ch as u8);
            }
            Some(KeyAction::Seq(seq)) => {
                for b in seq.bytes() {
                    KEYBOARD_BUFFER.push_overwrite(b);
                    tty::queue_input(b);
                }
            }
            None => {}
        }
    }
}

// ----------------------------------------------------------------------------
// Character Device (/dev/keyboard)
// ----------------------------------------------------------------------------

pub struct KeyboardDevice;

impl CharacterDevice for KeyboardDevice {
    fn name(&self) -> String {
        "keyboard".into()
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let mut count = 0;
        while count < buf.len() {
            if let Some(byte) = KEYBOARD_BUFFER.pop() {
                buf[count] = byte;
                count += 1;
            } else {
                break;
            }
        }
        Ok(count)
    }

    fn write(&self, buf: &[u8]) -> Result<usize> {
        Ok(buf.len())
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        if let Some(cons) = tty::get_console() {
            cons.ioctl(cmd, arg)
        } else {
            crate::return_errno!(ENOTTY, "inappropriate ioctl for device")
        }
    }

    fn poll(&self) -> Result<u32> {
        if let Some(cons) = tty::get_console() {
            cons.poll()
        } else {
            Ok(0)
        }
    }
}

// ----------------------------------------------------------------------------
// Initialization & Registration
// ----------------------------------------------------------------------------

/// Probes and initializes the PS/2 keyboard, binds IRQ 1, and registers the driver.
pub fn init(controller: &I8042Controller) -> Result<()> {
    ostd::info!("PS/2 Keyboard: Initializing...");

    // 1. Send reset command (non-fatal if keyboard ignores it)
    if let Err(err) = send_keyboard_cmd(controller, CMD_RESET) {
        ostd::warn!("PS/2 Keyboard: Reset command error (ignored): {:?}", err);
    }
    // Flush any self-test response (e.g. 0xAA)
    controller.flush_buffer();

    // 2. Enable keyboard scanning
    if let Err(err) = send_keyboard_cmd(controller, CMD_ENABLE_SCANNING) {
        ostd::warn!("PS/2 Keyboard: Enable scanning error (ignored): {:?}", err);
    }

    // 3. Allocate and map ISA IRQ 1
    if let Some(chip) = IRQ_CHIP.get() {
        if let Ok(irq_line) = IrqLine::alloc() {
            match chip.map_isa_pin_to(irq_line, ISA_KEYBOARD_IRQ) {
                Ok(mut mapped_line) => {
                    mapped_line.on_active(handle_keyboard_irq);
                    KEYBOARD_IRQ_LINE.call_once(|| mapped_line);
                    ostd::info!(
                        "PS/2 Keyboard: Mapped ISA IRQ {} successfully",
                        ISA_KEYBOARD_IRQ
                    );
                }
                Err(err) => {
                    ostd::warn!(
                        "PS/2 Keyboard: Failed to map ISA IRQ {}: {:?}",
                        ISA_KEYBOARD_IRQ,
                        err
                    );
                }
            }
        }
    }

    // 4. Register into unified driver registry
    crate::drivers::register_chrdev(Arc::new(KeyboardDevice));

    ostd::info!("PS/2 Keyboard: Driver registered successfully.");
    Ok(())
}
