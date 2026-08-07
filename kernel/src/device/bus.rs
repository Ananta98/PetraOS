//! Bus Abstractions

use super::device::Device;
use super::driver::DriverError;

pub trait Bus: Device {
    /// Probe devices attached to the bus
    fn probe(&mut self) -> Result<(), DriverError>;

    /// Remove devices from the bus
    fn remove(&mut self) -> Result<(), DriverError>;
}
