// SPDX-License-Identifier: GPL-2.0

//! PCI / PCIe Bus Subsystem.
//!
//! Organizes PCI functionalities into focused submodules:
//! - [`address`]: PCI device addressing (`Bus:Device.Function`).
//! - [`config`]: Low-level PCI configuration space port I/O and register definitions.
//! - [`bar`]: Base Address Register (BAR) decoding and probing.
//! - [`device`]: Discovered PCI device model and control helpers.
//! - [`driver`]: PCI driver registration on the generic device model bus.
//! - [`scan`]: PCI bus enumeration.
//! - [`ids`]: Standard PCI class and subclass IDs.

pub mod address;
pub mod bar;
pub mod config;
pub mod device;
pub mod driver;
pub mod ids;
pub mod scan;

use alloc::format;

pub use address::PciAddress;
pub use bar::{BarType, PciBar, probe_bar};
pub use config::{
    PCI_BAR0, PCI_CLASS_CODE, PCI_COMMAND, PCI_COMMAND_BUS_MASTER, PCI_COMMAND_INTX_DISABLE,
    PCI_COMMAND_IO_SPACE, PCI_COMMAND_MEMORY_SPACE, PCI_CONFIG_ADDRESS_PORT, PCI_CONFIG_DATA_PORT,
    PCI_DEVICE_ID, PCI_HEADER_TYPE, PCI_INTERRUPT_LINE, PCI_INTERRUPT_PIN, PCI_PROG_IF,
    PCI_REVISION_ID, PCI_STATUS, PCI_SUBCLASS, PCI_VENDOR_ID, pci_read_config_u8,
    pci_read_config_u16, pci_read_config_u32, pci_write_config_u8, pci_write_config_u16,
    pci_write_config_u32,
};
pub use device::PciDevice;
pub use driver::{PCI_ANY_ID, PciDeviceId, pci_register_driver, pci_unregister_driver};
pub use scan::scan_bus;

use crate::drivers::device::{Bus, BusHandle};
use crate::uapi::errno::Result;

/// The PCI bus on the generic device model.
pub struct PciBus;

impl Bus for PciBus {
    type Device = PciDevice;

    type MatchData = [PciDeviceId];

    fn matches(&self, device: &PciDevice, driver: &[PciDeviceId]) -> bool {
        driver.iter().any(|id| id.matches(device))
    }
}

/// Global registry of discovered PCI devices and their drivers.
pub static PCI_BUS: BusHandle<PciBus> = BusHandle::new(PciBus);

/// Initializes the PCI subsystem, enumerating all connected devices.
pub fn init() -> Result<()> {
    ostd::info!("Probing PCI bus...");
    let devices = scan_bus();

    ostd::info!("PCI: Discovered {} device(s)", devices.len());
    for payload in devices {
        ostd::info!(
            "PCI: [{}] {:04x}:{:04x} class {:02x}:{:02x} (prog_if {:02x})",
            payload.address,
            payload.vendor_id,
            payload.device_id,
            payload.class_code,
            payload.subclass,
            payload.prog_if
        );
        for bar in payload.bars.iter().flatten() {
            ostd::info!(
                "PCI: [{}] BAR{}: {:?} base {:#x} (size: {} KB)",
                payload.address,
                bar.index,
                bar.bar_type,
                bar.address,
                bar.size / 1024
            );
        }

        let name = format!("{}", payload.address);
        PCI_BUS.add_device(&name, payload)?;
    }

    Ok(())
}
