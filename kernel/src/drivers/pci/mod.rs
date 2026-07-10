mod bus;
mod capability;
/// PCI Subsystem for PetraOS
///
/// Provides PCI configuration space access, device enumeration,
/// BAR parsing, and capability list walking.
///
/// # Architecture
///
/// - `config` — Low-level config space read/write (I/O port mechanism 1 on x86_64)
/// - `device` — PCI device and BAR type definitions, config space convenience methods
/// - `bus`    — Bus enumeration and device discovery
/// - `capability` — PCI capability linked list traversal
mod config;
mod device;

pub use bus::{enumerate, find_device, find_devices_by_class};
pub use capability::{
    CAP_MSI, CAP_MSIX, CAP_PCIE, CAP_PM, CAP_VENDOR, PciCapability, capabilities,
};
pub use config::{
    config_read_u8, config_read_u16, config_read_u32, config_write_u8, config_write_u16,
    config_write_u32,
};
pub use device::{PciBar, PciDevice};
