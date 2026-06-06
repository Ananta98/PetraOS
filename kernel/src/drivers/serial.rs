use crate::arch::x86_64::{inb, outb};
use super::{DeviceDriver, CharDevice, DriverError};

pub struct SerialPort {
    port: u16,
}

impl SerialPort {
    /// Create a new serial port wrapper.
    pub const fn new(port: u16) -> Self {
        Self { port }
    }
}

impl DeviceDriver for SerialPort {
    fn name(&self) -> &'static str {
        "16550 UART Serial Port"
    }

    fn init(&mut self) -> Result<(), DriverError> {
        // SAFETY: Accessing CPU I/O ports for initializing the 16550 UART device is safe on x86_64 at boot.
        unsafe {
            outb(self.port + 1, 0x00); // Disable all interrupts
            outb(self.port + 3, 0x80); // Enable DLAB (set baud rate divisor)
            outb(self.port + 0, 0x03); // Set divisor to 3 (lo byte) 38400 baud
            outb(self.port + 1, 0x00); //                  (hi byte)
            outb(self.port + 3, 0x03); // 8 bits, no parity, one stop bit
            outb(self.port + 2, 0xC7); // Enable FIFO, clear them, with 14-byte threshold
            outb(self.port + 4, 0x0B); // IRQs enabled, RTS/DSR set
        }
        Ok(())
    }
}

impl CharDevice for SerialPort {
    fn read_byte(&mut self) -> Result<u8, DriverError> {
        // SAFETY: Reading from the configured serial port registers is safe.
        unsafe {
            while (inb(self.port + 5) & 1) == 0 {
                // Spin waiting for data to receive
            }
            Ok(inb(self.port))
        }
    }

    fn write_byte(&mut self, byte: u8) -> Result<(), DriverError> {
        // SAFETY: Writing to the configured serial port registers is safe.
        unsafe {
            while (inb(self.port + 5) & 0x20) == 0 {
                // Spin waiting for transmit buffer to empty
            }
            outb(self.port, byte);
        }
        Ok(())
    }
}
