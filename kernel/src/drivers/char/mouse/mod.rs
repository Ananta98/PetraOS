//! PS/2 Character Mouse Driver
//!
//! Handles 8042 auxiliary port communication, packet decoding,
//! relative movement tracking, and device registration for `/dev/input/mice`.

pub mod buffer;
pub mod packet;
pub mod ps2;

use crate::device::{CharDevice, Device, DeviceType, Driver, DriverError};
use crate::sync::Mutex;
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub use buffer::{MOUSE_RING_BUFFER, MOUSE_WAIT_QUEUE, MouseRingBuffer};
pub use packet::{MousePacket, MousePacketParser};
pub use ps2::Ps2MouseController;

/// Total number of mouse interrupts handled.
static MOUSE_INTERRUPT_COUNT: AtomicU64 = AtomicU64::new(0);

/// Global packet parser state machine.
static MOUSE_PARSER: Mutex<MousePacketParser> = Mutex::new(MousePacketParser::new());

/// Whether scroll wheel capability was detected.
static HAS_WHEEL: AtomicBool = AtomicBool::new(false);

/// The PS/2 Character Mouse Device.
pub struct Ps2Mouse;

impl Ps2Mouse {
    pub const fn new() -> Self {
        Self
    }
}

impl Device for Ps2Mouse {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Char
    }

    fn name(&self) -> &'static str {
        "PS/2 Mouse"
    }

    fn dev_name(&self) -> Option<&'static str> {
        Some("mice")
    }

    fn init(&mut self) -> Result<(), DriverError> {
        let has_wheel = Ps2MouseController::init_mouse()?;
        HAS_WHEEL.store(has_wheel, Ordering::Relaxed);
        MOUSE_PARSER.lock().set_has_wheel(has_wheel);
        Ok(())
    }

    fn as_char_device(&self) -> Option<&dyn CharDevice> {
        Some(self)
    }

    fn as_char_device_mut(&mut self) -> Option<&mut dyn CharDevice> {
        Some(self)
    }
}

impl CharDevice for Ps2Mouse {
    fn read_byte(&mut self) -> Result<u8, DriverError> {
        if let Some(byte) = MOUSE_RING_BUFFER.pop() {
            Ok(byte)
        } else {
            Err(DriverError::ReadFailed)
        }
    }

    fn write_byte(&mut self, _byte: u8) -> Result<(), DriverError> {
        Ok(())
    }

    fn has_input(&self) -> bool {
        !MOUSE_RING_BUFFER.is_empty()
    }

    fn wait_queue(&self) -> Option<&'static crate::sync::WaitQueue> {
        Some(&MOUSE_WAIT_QUEUE)
    }
}

/// Global driver structure for module registration.
#[derive(Default)]
pub struct Ps2MouseDriver;

impl Driver for Ps2MouseDriver {
    fn name(&self) -> &'static str {
        "ps2_mouse"
    }

    fn bus_name(&self) -> &'static str {
        "platform"
    }

    fn description(&self) -> &'static str {
        "PS/2 Character Mouse Driver"
    }

    fn probe(&self) -> Result<(), DriverError> {
        let mut mouse = Ps2Mouse::new();
        mouse.init()?;

        let dev_ref: Arc<Mutex<Box<dyn Device>>> = Arc::new(Mutex::new(Box::new(mouse)));
        crate::device::DEVICE_MANAGER.write().register(dev_ref);

        log::info!("[PS/2 Mouse] Driver probed and registered to DEVICE_MANAGER as /dev/mice.");
        Ok(())
    }
}

/// Dispatches a raw byte received from the mouse interrupt handler (IRQ 12).
pub fn handle_mouse_byte(byte: u8) {
    MOUSE_INTERRUPT_COUNT.fetch_add(1, Ordering::Relaxed);

    // Push raw byte into the ring buffer for standard /dev/input/mice readers
    MOUSE_RING_BUFFER.push(byte);

    // Also process through packet parser to maintain packet framing
    let _ = MOUSE_PARSER.lock().process_byte(byte);

    // Wake any userland threads waiting for mouse data
    MOUSE_WAIT_QUEUE.wake_all();
}

/// Get the count of mouse hardware interrupts received.
pub fn interrupt_count() -> u64 {
    MOUSE_INTERRUPT_COUNT.load(Ordering::Relaxed)
}

crate::MODULE_LICENSE!("GPL-2.0");
crate::MODULE_AUTHOR!("Ananta");
crate::MODULE_DESCRIPTION!("PS/2 Character Mouse Driver");
crate::MODULE_VERSION!("1.0.0");
crate::module_driver!(
    MOUSE_INITCALL,
    mouse_driver_init,
    "ps2_mouse",
    Ps2MouseDriver
);
