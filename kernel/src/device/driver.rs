//! Device Driver Abstraction and Error Definitions

use core::fmt;

/// Unified error type for all device driver operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    /// Driver or hardware initialization failed.
    InitFailed,
    /// A hardware read operation failed.
    ReadFailed,
    /// A hardware write operation failed.
    WriteFailed,
    /// Operation is not supported by this driver.
    Unsupported,
    /// Block number is out of range or invalid.
    InvalidBlock,
    /// Hardware operation timed out.
    Timeout,
    /// Device not found or not present.
    NoDevice,
    /// Resource allocation (memory, DMA) failed.
    AllocFailed,
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InitFailed => write!(f, "driver initialization failed"),
            Self::ReadFailed => write!(f, "hardware read failed"),
            Self::WriteFailed => write!(f, "hardware write failed"),
            Self::Unsupported => write!(f, "operation not supported"),
            Self::InvalidBlock => write!(f, "invalid block number"),
            Self::Timeout => write!(f, "hardware operation timed out"),
            Self::NoDevice => write!(f, "device not found"),
            Self::AllocFailed => write!(f, "resource allocation failed"),
        }
    }
}

/// Probe and initialization lifecycle trait for device drivers.
///
/// `Driver` governs the probe/bind/unbind lifecycle of a driver.
/// `Device` is the runtime identity and I/O interface of a discovered device.
pub trait Driver: Send + Sync {
    /// Return the driver's unique registration name.
    fn name(&self) -> &'static str;

    /// Return the target bus name (e.g. "pci", "isa", "platform").
    fn bus_name(&self) -> &'static str {
        "platform"
    }

    /// Return a human-readable driver description.
    fn description(&self) -> &'static str {
        ""
    }

    /// Probe hardware on the target bus and initialize any discovered devices.
    fn probe(&self) -> Result<(), DriverError>;

    /// Initialize the driver (defaults to calling `probe`).
    fn init(&mut self) -> Result<(), DriverError> {
        self.probe()
    }

    /// Shut down and release driver resources.
    fn destroy(&mut self) -> Result<(), DriverError> {
        Ok(())
    }
}
