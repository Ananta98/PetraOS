// SPDX-License-Identifier: GPL-2.0

//! PCI Driver Registration on the Generic Device Model.
//!
//! Drivers implement [`Driver<PciBus>`] and declare their supported hardware
//! through [`PciDeviceId`] tables supplied as the bus's match data.

use alloc::sync::Arc;

use super::{PciBus, PCI_BUS};
use crate::drivers::device::Driver;
use crate::uapi::errno::Result;

/// Wildcard value matching any PCI vendor, device, or class ID.
pub const PCI_ANY_ID: u16 = 0xFFFF;

/// Matches a PCI device based on Vendor ID, Device ID, and/or Class/Subclass/ProgIF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciDeviceId {
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u16,
    pub subclass: u16,
    pub prog_if: u16,
}

impl PciDeviceId {
    /// Creates a custom PCI device matcher.
    pub const fn new(
        vendor_id: u16,
        device_id: u16,
        class_code: u16,
        subclass: u16,
        prog_if: u16,
    ) -> Self {
        Self {
            vendor_id,
            device_id,
            class_code,
            subclass,
            prog_if,
        }
    }

    /// Match a specific device by its exact Vendor ID and Device ID.
    pub const fn match_device(vendor_id: u16, device_id: u16) -> Self {
        Self {
            vendor_id,
            device_id,
            class_code: PCI_ANY_ID,
            subclass: PCI_ANY_ID,
            prog_if: PCI_ANY_ID,
        }
    }

    /// Match any device by Class, Subclass, and Programming Interface.
    pub const fn match_class(class_code: u8, subclass: u8, prog_if: u8) -> Self {
        Self {
            vendor_id: PCI_ANY_ID,
            device_id: PCI_ANY_ID,
            class_code: class_code as u16,
            subclass: subclass as u16,
            prog_if: prog_if as u16,
        }
    }

    /// Tests whether a given `PciDevice` matches this device ID filter.
    pub fn matches(&self, dev: &super::PciDevice) -> bool {
        let vendor_match = self.vendor_id == PCI_ANY_ID || self.vendor_id == dev.vendor_id;
        let device_match = self.device_id == PCI_ANY_ID || self.device_id == dev.device_id;
        let class_match = self.class_code == PCI_ANY_ID || self.class_code as u8 == dev.class_code;
        let subclass_match = self.subclass == PCI_ANY_ID || self.subclass as u8 == dev.subclass;
        let prog_if_match = self.prog_if == PCI_ANY_ID || self.prog_if as u8 == dev.prog_if;

        vendor_match && device_match && class_match && subclass_match && prog_if_match
    }
}

/// Registers a PCI driver with the PCI bus.
///
/// The driver is probed against every unbound PCI device already on the bus;
/// later devices are probed against this driver when they are discovered.
pub fn pci_register_driver(driver: Arc<dyn Driver<PciBus>>) -> Result<()> {
    ostd::info!("PCI: Registering driver \"{}\"...", driver.name());
    PCI_BUS.register_driver(driver)?;
    Ok(())
}

/// Unregisters a PCI driver and detaches it from bound devices.
pub fn pci_unregister_driver(name: &str) -> Result<()> {
    PCI_BUS.unregister_driver(name)
}
