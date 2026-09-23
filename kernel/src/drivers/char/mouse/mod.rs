//! PS/2 Character Mouse Driver
//!
//! Handles 8042 auxiliary port communication, packet decoding,
//! relative movement tracking, and device registration for `/dev/input/mice`.

pub mod packet;
pub mod ps2;

use crate::device::{CharDevice, Device, DeviceType, Driver, DriverError};
use crate::sync::Mutex;
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub use crate::utils::ring_buffer::MouseRingBuffer;
pub use packet::{MousePacket, MousePacketParser};
pub use ps2::Ps2MouseController;

/// Global mouse raw byte ring buffer for `/dev/input/mice` and `/dev/psaux`.
pub static MOUSE_RING_BUFFER: MouseRingBuffer<512> = MouseRingBuffer::new();

/// Wait queue for blocking reads on mouse device.
pub static MOUSE_WAIT_QUEUE: crate::sync::WaitQueue = crate::sync::WaitQueue::new();

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

    // Process through packet parser to maintain packet framing and synchronization
    let maybe_packet = MOUSE_PARSER.lock().process_byte(byte);

    if let Some(packet) = maybe_packet {
        // Construct standard 3-byte PS/2 mouse packet for /dev/input/mice:
        // Byte 0: Flags (bit 0: L, bit 1: R, bit 2: M, bit 3: 1, bit 4: X sign, bit 5: Y sign)
        let mut flags = 0x08u8;
        if packet.left_button {
            flags |= 0x01;
        }
        if packet.right_button {
            flags |= 0x02;
        }
        if packet.middle_button {
            flags |= 0x04;
        }
        if packet.dx < 0 {
            flags |= 0x10;
        }
        if packet.dy < 0 {
            flags |= 0x20;
        }

        // Clamped 8-bit movement deltas for standard PS/2 packet format
        let dx_byte = (packet.dx.clamp(-127, 127) as i8) as u8;
        let dy_byte = (packet.dy.clamp(-127, 127) as i8) as u8;

        // Atomically push the 3 framed bytes into the ring buffer
        MOUSE_RING_BUFFER.push(flags);
        MOUSE_RING_BUFFER.push(dx_byte);
        MOUSE_RING_BUFFER.push(dy_byte);

        // Wake any userland threads waiting for mouse data
        MOUSE_WAIT_QUEUE.wake_all();
    }
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
