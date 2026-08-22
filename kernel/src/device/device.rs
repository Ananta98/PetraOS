//! Device Core Traits and Types

pub type Major = u32;
pub type Minor = u32;

/// Category of a registered device.
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

// ===== Device Trait =====

/// Common interface for every device registered in the kernel device manager.
pub trait Device: Send + Sync {
    /// Return the device major number.
    fn major(&self) -> Major {
        0
    }

    /// Return the device minor number.
    fn minor(&self) -> Minor {
        0
    }

    /// Return `(major, minor)` as a convenience tuple.
    fn dev_id(&self) -> (Major, Minor) {
        (self.major(), self.minor())
    }

    /// Return the category of this device.
    fn dev_type(&self) -> DeviceType;

    /// Return the user-friendly name of the device.
    fn name(&self) -> &'static str;

    /// Return the standard device node name in `/dev` (e.g. "sda", "nvme0n1", "fb0").
    ///
    /// If `None` (the default), this device is not automatically exposed in devfs.
    fn dev_name(&self) -> Option<&'static str> {
        None
    }

    /// Initialize the hardware device.
    fn init(&mut self) -> Result<(), super::driver::DriverError>;

    /// Cast to `BlockDevice` for block I/O (returns `None` if unsupported).
    fn as_block_device(&self) -> Option<&dyn BlockDevice> {
        None
    }

    /// Cast to `BlockDevice` mutably for block I/O (returns `None` if unsupported).
    fn as_block_device_mut(&mut self) -> Option<&mut dyn BlockDevice> {
        None
    }

    /// Cast to `CharDevice` for byte-level I/O (returns `None` if unsupported).
    fn as_char_device(&self) -> Option<&dyn CharDevice> {
        None
    }

    /// Cast to `CharDevice` mutably for byte-level I/O (returns `None` if unsupported).
    fn as_char_device_mut(&mut self) -> Option<&mut dyn CharDevice> {
        None
    }
}

// ===== CharDevice Trait =====

/// Byte-level I/O interface for character devices (e.g. serial ports, console).
pub trait CharDevice: Device {
    /// Read a single byte from the character device.
    fn read_byte(&mut self) -> Result<u8, super::driver::DriverError>;

    /// Write a single byte to the character device.
    fn write_byte(&mut self, byte: u8) -> Result<(), super::driver::DriverError>;
}

// ===== BlockDevice Trait =====

/// Block-level I/O interface for storage devices (e.g. AHCI, NVMe).
pub trait BlockDevice: Device {
    /// Read `buf.len()` bytes aligned to a block boundary starting at `block_id`.
    fn read_block(
        &mut self,
        block_id: u64,
        buf: &mut [u8],
    ) -> Result<usize, super::driver::DriverError>;

    /// Write `buf.len()` bytes aligned to a block boundary starting at `block_id`.
    fn write_block(
        &mut self,
        block_id: u64,
        buf: &[u8],
    ) -> Result<usize, super::driver::DriverError>;

    /// Block (sector) size in bytes.
    fn block_size(&self) -> usize;
}
