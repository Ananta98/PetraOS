//! Device Driver Abstraction and Error Definitions

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    InitFailed,
    ReadFailed,
    WriteFailed,
    Unsupported,
    InvalidBlock,
}

pub trait Driver: Send + Sync {
    /// Return driver name
    fn name(&self) -> &'static str;

    /// Return target bus name (e.g. "pci", "isa", "platform")
    fn bus_name(&self) -> &'static str {
        "pci"
    }

    /// Return human-readable driver description
    fn description(&self) -> &'static str {
        ""
    }

    /// Probe and initialize device hardware on the target bus
    fn probe(&self) -> Result<(), DriverError>;

    /// Initialize driver hardware
    fn init(&mut self) -> Result<(), DriverError> {
        self.probe()
    }

    /// Shutdown driver hardware
    fn destroy(&mut self) -> Result<(), DriverError> {
        Ok(())
    }
}
