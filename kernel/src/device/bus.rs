//! Bus Abstractions
//!
//! A `Bus` is a special device that can enumerate child devices attached to it
//! (e.g., PCI, USB, I2C). Drivers for individual bus types implement this trait
//! to plug into the kernel device model.
//!
//! NOTE: This is currently a stub reserved for future PCI/USB bus enumeration.
//! No buses are registered in the DEVICE_MANAGER yet.

use super::device::Device;
use super::driver::DriverError;

pub trait Bus: Device {
    /// Probe and enumerate devices attached to this bus.
    fn probe(&mut self) -> Result<(), DriverError>;

    /// Remove all devices from this bus.
    fn remove(&mut self) -> Result<(), DriverError>;
}
