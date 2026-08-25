//! PS/2 Character Keyboard Driver
//!
//! Handles 8042 PS/2 controller communication, scancode decoding,
//! key buffering, and device registration.

pub mod buffer;
pub mod ps2;
pub mod scancode;

use crate::device::{CharDevice, Device, DeviceType, Driver, DriverError};
use crate::sync::Mutex;
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};

pub use buffer::{KEY_RING_BUFFER, KeyBuffer};
pub use ps2::Ps2Controller;
pub use scancode::{KeyCode, KeyEvent, KeyState, Modifiers, ScancodeDecoder};

/// Total number of keyboard interrupts handled.
static KEYBOARD_INTERRUPT_COUNT: AtomicU64 = AtomicU64::new(0);

/// Global scancode decoder state.
static SCANCODE_DECODER: Mutex<ScancodeDecoder> = Mutex::new(ScancodeDecoder::new());

/// The PS/2 Character Keyboard Device.
pub struct Ps2Keyboard;

impl Ps2Keyboard {
    pub const fn new() -> Self {
        Self
    }
}

impl Device for Ps2Keyboard {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Char
    }

    fn name(&self) -> &'static str {
        "PS/2 Keyboard"
    }

    fn init(&mut self) -> Result<(), DriverError> {
        Ps2Controller::init_keyboard()?;
        crate::arch::interrupt::ioapic::unmask_isa_irq(1);
        Ok(())
    }

    fn as_char_device(&self) -> Option<&dyn CharDevice> {
        Some(self)
    }

    fn as_char_device_mut(&mut self) -> Option<&mut dyn CharDevice> {
        Some(self)
    }
}

impl CharDevice for Ps2Keyboard {
    fn read_byte(&mut self) -> Result<u8, DriverError> {
        if let Some(byte) = KEY_RING_BUFFER.pop() {
            Ok(byte)
        } else {
            Err(DriverError::ReadFailed)
        }
    }

    fn write_byte(&mut self, _byte: u8) -> Result<(), DriverError> {
        // Keyboard LED or command writing can be extended here
        Ok(())
    }
}

/// Global driver structure for module registration.
#[derive(Default)]
pub struct Ps2KeyboardDriver;

impl Driver for Ps2KeyboardDriver {
    fn name(&self) -> &'static str {
        "ps2_keyboard"
    }

    fn bus_name(&self) -> &'static str {
        "platform"
    }

    fn description(&self) -> &'static str {
        "PS/2 Keyboard Driver"
    }

    fn probe(&self) -> Result<(), DriverError> {
        let mut kbd = Ps2Keyboard::new();
        kbd.init()?;

        let dev_ref: Arc<Mutex<Box<dyn Device>>> = Arc::new(Mutex::new(Box::new(kbd)));
        crate::device::DEVICE_MANAGER.write().register(dev_ref);
        log::info!("[PS/2 Keyboard] Driver probed and registered to DEVICE_MANAGER.");
        Ok(())
    }
}

/// Dispatches a raw scancode received from the interrupt handler.
pub fn handle_scancode(scancode: u8) {
    KEYBOARD_INTERRUPT_COUNT.fetch_add(1, Ordering::Relaxed);

    let event_opt = SCANCODE_DECODER.lock().process_scancode(scancode);

    if let Some(event) = event_opt {
        if event.state == KeyState::Pressed {
            if let Some(ch) = event.ascii {
                KEY_RING_BUFFER.push(ch as u8);
            } else {
                match event.code {
                    KeyCode::Up => {
                        KEY_RING_BUFFER.push(0x1B);
                        KEY_RING_BUFFER.push(b'[');
                        KEY_RING_BUFFER.push(b'A');
                    }
                    KeyCode::Down => {
                        KEY_RING_BUFFER.push(0x1B);
                        KEY_RING_BUFFER.push(b'[');
                        KEY_RING_BUFFER.push(b'B');
                    }
                    KeyCode::Right => {
                        KEY_RING_BUFFER.push(0x1B);
                        KEY_RING_BUFFER.push(b'[');
                        KEY_RING_BUFFER.push(b'C');
                    }
                    KeyCode::Left => {
                        KEY_RING_BUFFER.push(0x1B);
                        KEY_RING_BUFFER.push(b'[');
                        KEY_RING_BUFFER.push(b'D');
                    }
                    KeyCode::Home => {
                        KEY_RING_BUFFER.push(0x1B);
                        KEY_RING_BUFFER.push(b'[');
                        KEY_RING_BUFFER.push(b'H');
                    }
                    KeyCode::End => {
                        KEY_RING_BUFFER.push(0x1B);
                        KEY_RING_BUFFER.push(b'[');
                        KEY_RING_BUFFER.push(b'F');
                    }
                    KeyCode::PageUp => {
                        KEY_RING_BUFFER.push(0x1B);
                        KEY_RING_BUFFER.push(b'[');
                        KEY_RING_BUFFER.push(b'5');
                        KEY_RING_BUFFER.push(b'~');
                    }
                    KeyCode::PageDown => {
                        KEY_RING_BUFFER.push(0x1B);
                        KEY_RING_BUFFER.push(b'[');
                        KEY_RING_BUFFER.push(b'6');
                        KEY_RING_BUFFER.push(b'~');
                    }
                    KeyCode::Insert => {
                        KEY_RING_BUFFER.push(0x1B);
                        KEY_RING_BUFFER.push(b'[');
                        KEY_RING_BUFFER.push(b'2');
                        KEY_RING_BUFFER.push(b'~');
                    }
                    KeyCode::Delete => {
                        KEY_RING_BUFFER.push(0x1B);
                        KEY_RING_BUFFER.push(b'[');
                        KEY_RING_BUFFER.push(b'3');
                        KEY_RING_BUFFER.push(b'~');
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Read a single decoded character from the keyboard input buffer if available.
pub fn read_char() -> Option<char> {
    KEY_RING_BUFFER.pop().map(|b| b as char)
}

/// Get the count of keyboard hardware interrupts received.
pub fn interrupt_count() -> u64 {
    KEYBOARD_INTERRUPT_COUNT.load(Ordering::Relaxed)
}

crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("Ananta98");
crate::MODULE_DESCRIPTION!("PS/2 Character Keyboard Driver");
crate::MODULE_VERSION!("1.0.0");
crate::module_driver!(
    KEYBOARD_INITCALL,
    keyboard_driver_init,
    "ps2_keyboard",
    Ps2KeyboardDriver
);
