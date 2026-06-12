pub mod block;
pub mod framebuffer;
pub mod pci;
pub mod serial;

pub type Major = u32;
pub type Minor = u32;

use crate::sync::spinlock::Spinlock;
use alloc::sync::Arc;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Char,
    Block,
    Network,
    Bus,
    Unknown,
}

use alloc::boxed::Box;

pub trait Device: Send + Sync {
    /// Return the device major number
    fn major(&self) -> Major { 0 }
    
    /// Return the device minor number
    fn minor(&self) -> Minor { 0 }
    
    /// Return the device type
    fn dev_type(&self) -> DeviceType;
    
    /// Return the user-friendly name of the device.
    fn name(&self) -> &'static str;
    
    /// Initialize the hardware device.
    fn init(&mut self) -> Result<(), DriverError>;

    /// Cast to BlockDevice if supported
    fn as_block_device(&self) -> Option<&dyn BlockDevice> { None }
    fn as_block_device_mut(&mut self) -> Option<&mut dyn BlockDevice> { None }
}

pub trait CharDevice: Device {
    /// Read a single byte from the character device.
    fn read_byte(&mut self) -> Result<u8, DriverError>;
    
    /// Write a single byte to the character device.
    fn write_byte(&mut self, byte: u8) -> Result<(), DriverError>;
}

pub trait BlockDevice: Device {
    /// Read a block of data.
    fn read_block(&mut self, block_id: u64, buf: &mut [u8]) -> Result<usize, DriverError>;
    
    /// Write a block of data.
    fn write_block(&mut self, block_id: u64, buf: &[u8]) -> Result<usize, DriverError>;
    
    /// Block size in bytes.
    fn block_size(&self) -> usize;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    InitFailed,
    ReadFailed,
    WriteFailed,
    Unsupported,
    InvalidBlock,
}

pub static DEVICE_MANAGER: Spinlock<DeviceManager> = Spinlock::new(DeviceManager::new());

pub struct DeviceManager {
    devices: Vec<Arc<Spinlock<Box<dyn Device>>>>,
}

impl DeviceManager {
    pub const fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    pub fn register(&mut self, device: Arc<Spinlock<Box<dyn Device>>>) {
        self.devices.push(device);
    }

    pub fn get_devices(&self) -> Vec<Arc<Spinlock<Box<dyn Device>>>> {
        self.devices.clone()
    }
}
