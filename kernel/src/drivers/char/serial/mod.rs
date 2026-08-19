use crate::device::{CharDevice, Device, DeviceType, DriverError};

pub mod mmio;
pub mod portio;

pub use mmio::MmioBackend;
pub use portio::PortIoBackend;

pub trait SerialBackend: Send + Sync {
    fn read_reg(&self, offset: u16) -> u8;
    fn write_reg(&self, offset: u16, val: u8);
}

pub struct SerialPort<B: SerialBackend> {
    backend: B,
}

impl<B: SerialBackend> SerialPort<B> {
    pub const fn new(backend: B) -> Self {
        Self { backend }
    }

    /// Try reading a single byte from the serial FIFO if available.
    pub fn try_read_byte(&self) -> Option<u8> {
        if (self.backend.read_reg(5) & 1) != 0 {
            Some(self.backend.read_reg(0))
        } else {
            None
        }
    }
}

impl<B: SerialBackend + Send + Sync> Device for SerialPort<B> {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Char
    }

    fn name(&self) -> &'static str {
        "16550 UART Serial Port"
    }

    fn init(&mut self) -> Result<(), DriverError> {
        self.backend.write_reg(1, 0x00); // Disable all interrupts
        self.backend.write_reg(3, 0x80); // Enable DLAB (set baud rate divisor)
        self.backend.write_reg(0, 0x03); // Set divisor to 3 (lo byte) 38400 baud
        self.backend.write_reg(1, 0x00); //                  (hi byte)
        self.backend.write_reg(3, 0x03); // 8 bits, no parity, one stop bit
        self.backend.write_reg(2, 0xC7); // Enable FIFO, clear them, with 14-byte threshold
        self.backend.write_reg(4, 0x0B); // IRQs enabled, RTS/DSR set
        Ok(())
    }
}

impl<B: SerialBackend + Send + Sync> CharDevice for SerialPort<B> {
    fn read_byte(&mut self) -> Result<u8, DriverError> {
        if let Some(byte) = self.try_read_byte() {
            Ok(byte)
        } else {
            Err(DriverError::ReadFailed)
        }
    }

    fn write_byte(&mut self, byte: u8) -> Result<(), DriverError> {
        while (self.backend.read_reg(5) & 0x20) == 0 {
            // Spin waiting for transmit buffer to empty
        }
        self.backend.write_reg(0, byte);
        Ok(())
    }
}
