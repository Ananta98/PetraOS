// SPDX-License-Identifier: GPL-2.0

//! Extensible Host Controller Interface (xHCI) USB 3.0 Driver.
//!
//! Provides PCI bus binding, host controller initialization, memory-mapped
//! I/O management, and root hub enumeration for xHCI controllers.

pub mod controller;
pub mod probe;
pub mod regs;
pub mod ring;

use alloc::sync::Arc;
use ostd::io::IoMem;

use crate::{
    drivers::{
        bus::pci::{PciBus, PciDeviceId, pci_register_driver, self},
        device::{BusDevice, Driver},
    },
    uapi::errno::{Errno, Result},
};

pub use controller::XhciController;
pub use probe::XHCI_DEVICES;

// ----------------------------------------------------------------------------
// PCI Driver Implementation
// ----------------------------------------------------------------------------

pub struct XhciPciDriver;

static XHCI_PCI_IDS: [PciDeviceId; 1] = [PciDeviceId::match_class(
    pci::ids::class::SERIAL_BUS,
    pci::ids::serial_bus::USB,
    pci::ids::serial_bus::usb::XHCI,
)];

impl Driver<PciBus> for XhciPciDriver {
    fn name(&self) -> &'static str {
        "xhci_hcd"
    }

    fn match_data(&self) -> &[PciDeviceId] {
        &XHCI_PCI_IDS
    }

    fn on_probe(&self, device: &BusDevice<PciBus>) -> Result<()> {
        ostd::info!(
            "xHCI: Initializing USB Host Controller at PCI {}",
            device.address
        );

        // 1. Enable Bus Mastering (DMA) and Memory Space
        device.enable_bus_mastering();
        device.enable_memory_space();

        // 2. Locate BAR0 (MMIO Base for Capability & Operational Registers)
        let bar0 = device.bars[0].ok_or_else(|| {
            ostd::error!("xHCI: Controller at {} has no valid BAR0", device.address);
            Errno::ENODEV
        })?;

        let mmio_range = (bar0.address as usize)..(bar0.address as usize + bar0.size as usize);
        let mmio = IoMem::acquire(mmio_range).map_err(|_| {
            ostd::error!(
                "xHCI: Failed to map BAR0 MMIO region at {:#X}",
                bar0.address
            );
            Errno::EIO
        })?;

        // 3. Initialize Controller and data structures
        let mut controller = controller::XhciController::new(mmio, bar0.address as usize)?;

        // 4. Probe Root Hub ports and discover connected USB devices
        probe::probe_root_hub(&mut controller)?;

        ostd::info!(
            "xHCI: Controller at {} initialized and running",
            device.address
        );
        Ok(())
    }
}

// ----------------------------------------------------------------------------
// Kernel Module Declaration
// ----------------------------------------------------------------------------

fn init() -> Result<()> {
    pci_register_driver(Arc::new(XhciPciDriver))
}

crate::module!(
    "xHCI USB Host Controller Driver",
    "Ananta98",
    crate::modules::InitcallLevel::Device,
    init
);
