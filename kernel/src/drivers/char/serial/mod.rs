use crate::device::{CharDevice, Device, DeviceType, Driver, DriverError};
use crate::sync::Mutex;
use alloc::boxed::Box;
use alloc::sync::Arc;

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
        if self.is_rx_ready() {
            Some(self.backend.read_reg(0))
        } else {
            None
        }
    }

    /// Returns true if a received byte is waiting in the FIFO.
    pub fn is_rx_ready(&self) -> bool {
        (self.backend.read_reg(5) & 1) != 0
    }

    /// Returns true if the transmitter FIFO/holding register is empty and ready for new data.
    pub fn is_tx_ready(&self) -> bool {
        (self.backend.read_reg(5) & 0x20) != 0
    }
}

impl<B: SerialBackend + Send + Sync + 'static> Device for SerialPort<B> {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Char
    }

    fn name(&self) -> &'static str {
        "16550 UART Serial Port"
    }

    fn dev_name(&self) -> Option<&'static str> {
        Some("ttyS0")
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

    fn as_char_device(&self) -> Option<&dyn CharDevice> {
        Some(self)
    }

    fn as_char_device_mut(&mut self) -> Option<&mut dyn CharDevice> {
        Some(self)
    }
}

impl<B: SerialBackend + Send + Sync + 'static> CharDevice for SerialPort<B> {
    fn read_byte(&mut self) -> Result<u8, DriverError> {
        if let Some(byte) = self.try_read_byte() {
            Ok(byte)
        } else {
            Err(DriverError::ReadFailed)
        }
    }

    fn write_byte(&mut self, byte: u8) -> Result<(), DriverError> {
        while !self.is_tx_ready() {
            // Spin waiting for transmit buffer to empty
            core::hint::spin_loop();
        }
        self.backend.write_reg(0, byte);
        Ok(())
    }
}

/// Global serial port driver structure for module registration.
#[derive(Default)]
pub struct SerialDriver;

impl Driver for SerialDriver {
    fn name(&self) -> &'static str {
        "serial"
    }

    fn bus_name(&self) -> &'static str {
        "isa"
    }

    fn description(&self) -> &'static str {
        "16550 UART Serial Driver"
    }

    fn probe(&self) -> Result<(), DriverError> {
        let mut port = SerialPort::new(PortIoBackend::new(0x3F8));
        port.init()?;

        let dev_ref: Arc<Mutex<Box<dyn Device>>> = Arc::new(Mutex::new(Box::new(port)));
        crate::device::DEVICE_MANAGER.write().register(dev_ref);
        log::info!("[Serial] 16550 UART COM1 probed and registered to DEVICE_MANAGER as ttyS0");
        Ok(())
    }
}

crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("Ananta98");
crate::MODULE_DESCRIPTION!("16550 UART Serial Driver");
crate::MODULE_VERSION!("1.0.0");
crate::module_driver!(
    SERIAL_INITCALL,
    serial_driver_init,
    "serial",
    SerialDriver
);
