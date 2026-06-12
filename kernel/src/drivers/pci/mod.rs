pub mod config;
pub mod device;
pub mod bus;

pub use device::PciDevice;
pub use bus::{PciDiscovery, PciBus};

use crate::drivers::DriverError;

pub fn enumerate() -> PciDiscovery {
    PciBus::enumerate()
}

pub fn init() -> Result<(), DriverError> {
    PciBus::init()?;
    Ok(())
}
