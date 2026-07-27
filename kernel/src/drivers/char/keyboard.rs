use super::{CharDevice, InputBuffer, register_char_device};
use alloc::sync::Arc;
use ostd::arch::device::io_port::ReadWriteAccess;
use ostd::arch::irq::{IRQ_CHIP, MappedIrqLine};
use ostd::arch::trap::TrapFrame;
use ostd::io::IoPort;
use ostd::irq::IrqLine;
use ostd::sync::SpinLock;
use pc_keyboard::{DecodedKey, HandleControl, Keyboard as PcKeyboard, ScancodeSet1, layouts};
use spin::Once;

/// Default capacity (in bytes) of the keyboard's internal character buffer.
const DEFAULT_BUFFER_CAPACITY: usize = 4096;

/// A character device that decodes PS/2 (Scan Code Set 1) scancodes pushed by
/// the keyboard interrupt handler into ASCII/UTF-8 text using the `pc-keyboard` crate.
///
/// Use [`Keyboard::push_scancode`] from the keyboard ISR to feed raw
/// scancodes; the decoder translates them and enqueues the resulting characters.
/// User-space reads the decoded stream via `/dev/keyboard`, which drains the buffer.
pub struct Keyboard {
    buf: InputBuffer,
    decoder: SpinLock<PcKeyboard<layouts::Us104Key, ScancodeSet1>>,
}

impl Keyboard {
    /// Create a new keyboard device with the default buffer capacity (4096 bytes).
    pub fn new() -> Self {
        Self {
            buf: InputBuffer::new(DEFAULT_BUFFER_CAPACITY),
            decoder: SpinLock::new(PcKeyboard::new(
                ScancodeSet1::new(),
                layouts::Us104Key,
                HandleControl::MapLettersToUnicode,
            )),
        }
    }

    /// Feed a single raw scancode byte from the keyboard ISR.
    ///
    /// The `pc-keyboard` decoder handles scancode state machine, modifier state,
    /// and multi-byte extended key sequences internally.
    pub fn push_scancode(&self, scancode: u8) {
        let mut decoder = self.decoder.lock();
        if let Ok(Some(key_event)) = decoder.add_byte(scancode) {
            if let Some(key) = decoder.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(ch) => {
                        let mut tmp = [0u8; 4];
                        let s = ch.encode_utf8(&mut tmp);
                        self.buf.push(s.as_bytes());
                    }
                    DecodedKey::RawKey(_raw) => {}
                }
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
}

impl Default for Keyboard {
    fn default() -> Self {
        Self::new()
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

const ISA_INTR_NUM: u8 = 1;

static IRQ_LINE: Once<MappedIrqLine> = Once::new();
static KEYBOARD_DEV: Once<Arc<Keyboard>> = Once::new();

fn handle_keyboard_input(_trap_frame: &TrapFrame) {
    let Ok(port) = IoPort::<u8, ReadWriteAccess>::acquire_overlapping(0x60) else {
        return;
    };
    if let Some(dev) = KEYBOARD_DEV.get() {
        dev.push_scancode(port.read());
    }
}

// ---------------------------------------------------------------------------
// Initialisation
// ---------------------------------------------------------------------------

pub struct KeyboardDriver;

impl crate::device::Driver for KeyboardDriver {
    fn name(&self) -> &str {
        "keyboard"
    }

    fn bus_name(&self) -> &str {
        "platform"
    }

    fn description(&self) -> &str {
        "PS/2 Keyboard Input Device Driver"
    }

    fn probe(&self) -> Result<(), ostd::Error> {
        let keyboard = Arc::new(Keyboard::new());
        KEYBOARD_DEV.call_once(|| keyboard.clone());

        if let Ok(mut irq_line) = IrqLine::alloc().and_then(|irq_line| {
            IRQ_CHIP
                .get()
                .unwrap()
                .map_isa_pin_to(irq_line, ISA_INTR_NUM)
        }) {
            irq_line.on_active(handle_keyboard_input);
            IRQ_LINE.call_once(|| irq_line);
        }

        let _ = register_char_device("keyboard", keyboard);
        Ok(())
    }
}

crate::module_driver!(KEYBOARD_INITCALL, keyboard_driver_init, "keyboard", KeyboardDriver);

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    fn type_keys(codes: &[u8]) -> alloc::string::String {
        let kb = Keyboard::new();
        kb.push_scancodes(codes);
        let mut buf = [0u8; 64];
        let n = kb.read(&mut buf).unwrap();
        alloc::string::String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    #[ktest]
    fn test_plain_lowercase() {
        let s = type_keys(&[0x23, 0x12, 0x26, 0x26, 0x18]);
        assert_eq!(s, "hello");
    }

    #[ktest]
    fn test_shift_produces_uppercase_and_symbols() {
        let s = type_keys(&[0x2A, 0x1E, 0xAA]);
        assert_eq!(s, "A");

        let s = type_keys(&[0x2A, 0x02, 0xAA]);
        assert_eq!(s, "!");
    }

    #[ktest]
    fn test_caps_lock_toggles_case() {
        let kb = Keyboard::new();
        kb.push_scancodes(&[0x3A, 0xBA, 0x1E]);
        let mut buf = [0u8; 64];
        let n = kb.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"A");
        kb.push_scancodes(&[0x3A, 0xBA, 0x1E]);
        let n = kb.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"a");
    }

    #[ktest]
    fn test_special_keys() {
        let s = type_keys(&[0x1C, 0x39, 0x0F]);
        assert_eq!(s, "\n \t");
    }

    #[ktest]
    fn test_extended_key_emits_no_ascii() {
        let s = type_keys(&[0xE0, 0x48]);
        assert_eq!(s, "");
        assert_eq!(Keyboard::new().available(), 0);
    }
}
