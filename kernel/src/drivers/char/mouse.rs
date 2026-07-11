use super::{CharDevice, InputBuffer, register_char_device};
use alloc::sync::Arc;

/// Default capacity (in bytes) of the mouse's internal packet buffer.
const DEFAULT_BUFFER_CAPACITY: usize = 4096;

/// A character device that exposes mouse-packet bytes received from the PS/2
/// or USB HID mouse interrupt handler.
///
/// Use [`Mouse::push_packet`] from the mouse ISR to enqueue data;
/// user-space reads via `/dev/mouse` drain the buffer.
pub struct Mouse {
    buf: InputBuffer,
}

impl Mouse {
    /// Create a new mouse device with the default buffer capacity.
    pub fn new() -> Self {
        Self {
            buf: InputBuffer::new(DEFAULT_BUFFER_CAPACITY),
        }
    }

    /// Enqueue a mouse packet (to be called by the mouse ISR).
    pub fn push_packet(&self, packet: &[u8]) {
        self.buf.push(packet);
    }

    /// Return the number of buffered packet bytes.
    pub fn available(&self) -> usize {
        self.buf.available()
    }
}

impl CharDevice for Mouse {
    fn read(&self, buf: &mut [u8]) -> Result<usize, ostd::Error> {
        self.buf.read_into(buf)
    }

    fn write(&self, _buf: &[u8]) -> Result<usize, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
}

/// Register the mouse device with devfs.
pub fn init() {
    let mouse = Arc::new(Mouse::new());
    let _ = register_char_device("mouse", mouse);
}
