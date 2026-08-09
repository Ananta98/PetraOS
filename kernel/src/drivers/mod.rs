//! Device Drivers Subsystem

pub mod block;
pub mod bus;
pub mod char;
pub mod gpu;
pub mod net;

pub use bus::pci;
pub use char::serial;

/// Initialize device drivers and PCI bus enumeration.
pub fn init() {
    log::info!("Initializing PCI bus and attached device drivers...");
    if let Err(e) = bus::pci::bus::PciBus::init() {
        log::warn!("PCI Bus initialization warning: {:?}", e);
    }
}
