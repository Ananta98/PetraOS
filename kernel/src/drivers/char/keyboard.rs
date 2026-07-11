use super::{CharDevice, InputBuffer, register_char_device};
use alloc::sync::Arc;
use ostd::sync::SpinLock;

/// Default capacity (in bytes) of the keyboard's internal character buffer.
const DEFAULT_BUFFER_CAPACITY: usize = 4096;

/// Keyboard modifier (Shift / Ctrl / Alt / Caps-Lock) state.
///
/// Modifier state is tracked by the decoder as scancodes arrive so that
/// decoded output reflects the currently held modifier keys.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Left Shift key currently held.
    pub left_shift: bool,
    /// Right Shift key currently held.
    pub right_shift: bool,
    /// Control key currently held (left or right).
    pub ctrl: bool,
    /// Alt key currently held (left or right).
    pub alt: bool,
    /// Caps Lock toggle state.
    pub caps_lock: bool,
}

impl Modifiers {
    /// Whether either Shift key is currently held.
    pub fn is_shift_active(&self) -> bool {
        self.left_shift || self.right_shift
    }
}

/// Internal decoder state machine for the PS/2 Scan Code Set 1 protocol.
///
/// Scancodes may arrive one byte at a time from the keyboard ISR, so the
/// state needed to interpret multi-byte sequences (the `0xE0` extended
/// prefix and the `0xF0` key-up prefix) and to track held modifiers must
/// persist across [`Keyboard::push_scancode`] calls.
struct DecoderState {
    /// The previous byte was the `0xE0` extended-key prefix.
    extended: bool,
    /// The previous byte was the `0xF0` key-up prefix.
    expect_break: bool,
    /// Currently held modifiers.
    modifiers: Modifiers,
    /// Num Lock toggle state (affects the numeric keypad).
    num_lock: bool,
}

impl Default for DecoderState {
    fn default() -> Self {
        Self {
            extended: false,
            expect_break: false,
            modifiers: Modifiers::default(),
            num_lock: true,
        }
    }
}

/// A character device that decodes PS/2 (Scan Code Set 1) scancodes pushed by
/// the keyboard interrupt handler into ASCII text.
///
/// Use [`Keyboard::push_scancode`] from the keyboard ISR to feed raw
/// scancodes; the decoder translates them (honouring Shift, Ctrl, Alt and
/// Caps Lock) and enqueues the resulting characters. User-space reads the
/// decoded stream via `/dev/keyboard`, which drains the buffer.
pub struct Keyboard {
    buf: InputBuffer,
    state: SpinLock<DecoderState>,
}

impl Keyboard {
    /// Create a new keyboard device with the default buffer capacity (4096 bytes).
    pub fn new() -> Self {
        Self {
            buf: InputBuffer::new(DEFAULT_BUFFER_CAPACITY),
            state: SpinLock::new(DecoderState::default()),
        }
    }

    /// Feed a single raw scancode byte from the keyboard ISR.
    ///
    /// Multi-byte sequences are assembled internally: a `0xE0` byte marks the
    /// following code as an "extended" key, and a `0xF0` byte marks the
    /// following code as a key *release*. On a key *press* of a printable key,
    /// the decoded ASCII character is enqueued into the read buffer.
    pub fn push_scancode(&self, scancode: u8) {
        let mut state = self.state.lock();

        match scancode {
            0xE0 => {
                state.extended = true;
                return;
            }
            0xF0 => {
                state.expect_break = true;
                return;
            }
            _ => {}
        }

        let code = scancode;
        let pressed = !state.expect_break;
        let extended = state.extended;

        // Prefixes only affect this one code; clear them for the next byte.
        state.extended = false;
        state.expect_break = false;

        // Modifier keys update tracked state but emit no character themselves.
        match (extended, code) {
            (false, 0x2A) => state.modifiers.left_shift = pressed,
            (false, 0x36) => state.modifiers.right_shift = pressed,
            (false, 0x1D) | (true, 0x1D) => state.modifiers.ctrl = pressed,
            (false, 0x38) | (true, 0x38) => state.modifiers.alt = pressed,
            (false, 0x3A) if pressed => state.modifiers.caps_lock = !state.modifiers.caps_lock,
            (false, 0x45) if pressed => state.num_lock = !state.num_lock,
            _ => {}
        }

        // Only key presses (make codes) of non-modifier keys produce output.
        if !pressed || matches!(code, 0x2A | 0x36 | 0x1D | 0x38 | 0x3A | 0x45 | 0x46) {
            return;
        }

        // Extended keys (e.g. arrows, preceded by 0xE0) have no ASCII
        // representation in Scan Code Set 1, so only decode non-extended codes.
        if !extended {
            if let Some(ch) = decode_char(code, &state.modifiers) {
                self.buf.push(&[ch]);
            }
        }
    }

    /// Feed a slice of raw scancode bytes (convenience wrapper around
    /// [`Keyboard::push_scancode`] for ISRs that batch reads).
    pub fn push_scancodes(&self, scancodes: &[u8]) {
        for &scancode in scancodes {
            self.push_scancode(scancode);
        }
    }

    /// Return the number of decoded characters buffered for reading.
    pub fn available(&self) -> usize {
        self.buf.available()
    }

    /// Snapshot of the current modifier state (handy for consumers that want
    /// to know which modifiers were held for the most recent key event).
    pub fn modifiers(&self) -> Modifiers {
        self.state.lock().modifiers
    }
}

/// Map a Scan Code Set 1 *make* code to its `(unshifted, shifted)` ASCII pair.
/// Returns `None` for keys that have no textual representation (function keys,
/// modifier keys, etc.). The numeric keypad codes are mapped to their digits
/// (valid when Num Lock is on, which is the default state).
fn ascii_pair(code: u8) -> Option<(u8, u8)> {
    Some(match code {
        0x01 => (0x1B, 0x1B), // Escape
        0x02 => (b'1', b'!'),
        0x03 => (b'2', b'@'),
        0x04 => (b'3', b'#'),
        0x05 => (b'4', b'$'),
        0x06 => (b'5', b'%'),
        0x07 => (b'6', b'^'),
        0x08 => (b'7', b'&'),
        0x09 => (b'8', b'*'),
        0x0A => (b'9', b'('),
        0x0B => (b'0', b')'),
        0x0C => (b'-', b'_'),
        0x0D => (b'=', b'+'),
        0x0E => (0x08, 0x08), // Backspace
        0x0F => (0x09, 0x09), // Tab
        0x10 => (b'q', b'Q'),
        0x11 => (b'w', b'W'),
        0x12 => (b'e', b'E'),
        0x13 => (b'r', b'R'),
        0x14 => (b't', b'T'),
        0x15 => (b'y', b'Y'),
        0x16 => (b'u', b'U'),
        0x17 => (b'i', b'I'),
        0x18 => (b'o', b'O'),
        0x19 => (b'p', b'P'),
        0x1A => (b'[', b'{'),
        0x1B => (b']', b'}'),
        0x1C => (0x0A, 0x0A), // Enter (newline)
        0x1E => (b'a', b'A'),
        0x1F => (b's', b'S'),
        0x20 => (b'd', b'D'),
        0x21 => (b'f', b'F'),
        0x22 => (b'g', b'G'),
        0x23 => (b'h', b'H'),
        0x24 => (b'j', b'J'),
        0x25 => (b'k', b'K'),
        0x26 => (b'l', b'L'),
        0x27 => (b';', b':'),
        0x28 => (b'\'', b'"'),
        0x29 => (b'`', b'~'),
        0x2B => (b'\\', b'|'),
        0x2C => (b'z', b'Z'),
        0x2D => (b'x', b'X'),
        0x2E => (b'c', b'C'),
        0x2F => (b'v', b'V'),
        0x30 => (b'b', b'B'),
        0x31 => (b'n', b'N'),
        0x32 => (b'm', b'M'),
        0x33 => (b',', b'<'),
        0x34 => (b'.', b'>'),
        0x35 => (b'/', b'?'),
        0x37 => (b'*', b'*'),
        0x39 => (b' ', b' '), // Space
        0x47 => (b'7', b'7'), // Numeric keypad
        0x48 => (b'8', b'8'),
        0x49 => (b'9', b'9'),
        0x4B => (b'4', b'4'),
        0x4C => (b'5', b'5'),
        0x4D => (b'6', b'6'),
        0x4F => (b'1', b'1'),
        0x50 => (b'2', b'2'),
        0x51 => (b'3', b'3'),
        0x52 => (b'0', b'0'),
        0x53 => (b'.', b'.'),
        _ => return None,
    })
}

/// Decode a make code into a single ASCII byte, applying Shift and Caps Lock.
fn decode_char(code: u8, modifiers: &Modifiers) -> Option<u8> {
    let (base, shifted) = ascii_pair(code)?;
    let mut ch = if modifiers.is_shift_active() {
        shifted
    } else {
        base
    };
    if modifiers.caps_lock {
        ch = toggle_case(ch);
    }
    Some(ch)
}

/// Flip the letter-case of an ASCII byte (no-op for non-letters).
fn toggle_case(c: u8) -> u8 {
    match c {
        b'a'..=b'z' => c - (b'a' - b'A'),
        b'A'..=b'Z' => c + (b'a' - b'A'),
        _ => c,
    }
}

impl CharDevice for Keyboard {
    fn read(&self, buf: &mut [u8]) -> Result<usize, ostd::Error> {
        self.buf.read_into(buf)
    }

    fn write(&self, _buf: &[u8]) -> Result<usize, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
}

/// Register the keyboard device with devfs.
pub fn init() {
    let keyboard = Arc::new(Keyboard::new());
    let _ = register_char_device("keyboard", keyboard);
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    /// Helper: feed a sequence of make codes and return the decoded string.
    fn type_keys(codes: &[u8]) -> alloc::string::String {
        let kb = Keyboard::new();
        kb.push_scancodes(codes);
        let mut buf = [0u8; 64];
        let n = kb.read(&mut buf).unwrap();
        alloc::string::String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    #[ktest]
    fn test_plain_lowercase() {
        // 'h'(0x23) 'e'(0x12) 'l'(0x26) 'l'(0x26) 'o'(0x18)
        let s = type_keys(&[0x23, 0x12, 0x26, 0x26, 0x18]);
        assert_eq!(s, "hello");
    }

    #[ktest]
    fn test_shift_produces_uppercase_and_symbols() {
        // LShift down(0x2A) 'a'(0x1E) LShift up(0xF0 0x2A)
        let s = type_keys(&[0x2A, 0x1E, 0xF0, 0x2A]);
        assert_eq!(s, "A");

        // '1'(0x02) with shift -> '!'
        let s = type_keys(&[0x2A, 0x02, 0xF0, 0x2A]);
        assert_eq!(s, "!");
    }

    #[ktest]
    fn test_caps_lock_toggles_case() {
        let kb = Keyboard::new();
        // CapsLock down(0x3A) up(0xF0 0x3A) then 'a'(0x1E) -> 'A'
        kb.push_scancodes(&[0x3A, 0xF0, 0x3A, 0x1E]);
        let mut buf = [0u8; 64];
        let n = kb.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"A");
        // Toggle Caps Lock off, then 'a'(0x1E) -> 'a'
        kb.push_scancodes(&[0x3A, 0xF0, 0x3A, 0x1E]);
        let n = kb.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"a");
    }

    #[ktest]
    fn test_special_keys() {
        // Enter(0x1C) -> '\n', Space(0x39) -> ' ', Tab(0x0F) -> '\t'
        let s = type_keys(&[0x1C, 0x39, 0x0F]);
        assert_eq!(s, "\n \t");
    }

    #[ktest]
    fn test_extended_key_emits_no_ascii() {
        // Arrow Up is 0xE0 0x48; it has no ASCII mapping.
        let s = type_keys(&[0xE0, 0x48]);
        assert_eq!(s, "");
        assert_eq!(Keyboard::new().available(), 0);
    }
}
