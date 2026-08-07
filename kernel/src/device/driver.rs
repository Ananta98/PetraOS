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

    /// Initialize driver hardware
    fn init(&mut self) -> Result<(), DriverError>;

    /// Shutdown driver hardware
    fn destroy(&mut self) -> Result<(), DriverError> {
        Ok(())
    }
}
