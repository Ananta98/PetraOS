pub trait DeviceDriver {
    /// Return the user-friendly name of the device.
    fn name(&self) -> &'static str;
    
    /// Initialize the hardware device.
    fn init(&mut self) -> Result<(), DriverError>;
}

pub trait CharDevice: DeviceDriver {
    /// Read a single byte from the character device.
    fn read_byte(&mut self) -> Result<u8, DriverError>;
    
    /// Write a single byte to the character device.
    fn write_byte(&mut self, byte: u8) -> Result<(), DriverError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    InitFailed,
    ReadFailed,
    WriteFailed,
}

pub mod serial;
pub mod framebuffer;
