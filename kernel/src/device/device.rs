//! Device Core Traits and Types

pub type Major = u32;
pub type Minor = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Char,
    Block,
    Network,
    Bus,
    Gpu,
    Audio,
    Unknown,
}

pub trait Device: Send + Sync {
    /// Return the device major number
    fn major(&self) -> Major {
        0
    }

    /// Return the device minor number
    fn minor(&self) -> Minor {
        0
    }

    /// Return the device type
    fn dev_type(&self) -> DeviceType;

    /// Return the user-friendly name of the device.
    fn name(&self) -> &'static str;

    /// Initialize the hardware device.
    fn init(&mut self) -> Result<(), super::driver::DriverError>;

    /// Cast to BlockDevice if supported
    fn as_block_device(&self) -> Option<&dyn BlockDevice> {
        None
    }
    fn as_block_device_mut(&mut self) -> Option<&mut dyn BlockDevice> {
        None
    }

    /// Cast to CharDevice if supported
    fn as_char_device(&self) -> Option<&dyn CharDevice> {
        None
    }
    fn as_char_device_mut(&mut self) -> Option<&mut dyn CharDevice> {
        None
    }
}

pub trait CharDevice: Device {
    /// Read a single byte from the character device.
    fn read_byte(&mut self) -> Result<u8, super::driver::DriverError>;

    /// Write a single byte to the character device.
    fn write_byte(&mut self, byte: u8) -> Result<(), super::driver::DriverError>;
}

pub trait BlockDevice: Device {
    /// Read a block of data.
    fn read_block(
        &mut self,
        block_id: u64,
        buf: &mut [u8],
    ) -> Result<usize, super::driver::DriverError>;

    /// Write a block of data.
    fn write_block(
        &mut self,
        block_id: u64,
        buf: &[u8],
    ) -> Result<usize, super::driver::DriverError>;

    /// Block size in bytes.
    fn block_size(&self) -> usize;
}
