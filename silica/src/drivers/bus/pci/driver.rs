// SPDX-License-Identifier: GPL-2.0

//! PCI Driver Model & Registration Subsystem.
//!
//! Provides a Linux-compatible PCI driver registration and device binding model.
//! Drivers implement [`PciDriver`] and declare their supported hardware via [`PciDeviceId`] tables.

use alloc::{sync::Arc, vec::Vec};
use spin::Mutex;

use super::device::PciDevice;
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
    pub fn matches(&self, dev: &PciDevice) -> bool {
        let vendor_match = self.vendor_id == PCI_ANY_ID || self.vendor_id == dev.vendor_id;
        let device_match = self.device_id == PCI_ANY_ID || self.device_id == dev.device_id;
        let class_match = self.class_code == PCI_ANY_ID || self.class_code as u8 == dev.class_code;
        let subclass_match = self.subclass == PCI_ANY_ID || self.subclass as u8 == dev.subclass;
        let prog_if_match = self.prog_if == PCI_ANY_ID || self.prog_if as u8 == dev.prog_if;

        vendor_match && device_match && class_match && subclass_match && prog_if_match
    }
}

/// Trait implemented by all PCI drivers in PetraOS.
pub trait PciDriver: Send + Sync {
    /// Human-readable name of the driver (e.g., "nvme", "e1000").
    fn name(&self) -> &'static str;

    /// Table of supported PCI device IDs.
    fn id_table(&self) -> &[PciDeviceId];

    /// Probes and initializes a matching, unbound PCI device.
    fn probe(&self, dev: &PciDevice) -> Result<()>;

    /// Cleans up device resources when the device is removed or driver is unloaded.
    fn remove(&self, _dev: &PciDevice) -> Result<()> {
        Ok(())
    }
}

/// Global list of registered PCI drivers.
pub static PCI_DRIVERS: Mutex<Vec<Arc<dyn PciDriver>>> = Mutex::new(Vec::new());

/// Registers a PCI driver with the PCI bus subsystem.
///
/// When a driver is registered, the PCI core matches it against all already
/// enumerated and unbound PCI devices, invoking `probe()` on each match.
pub fn pci_register_driver(driver: Arc<dyn PciDriver>) -> Result<()> {
    ostd::info!("PCI: Registering driver \"{}\"...", driver.name());

    let matching: Vec<PciDevice> = {
        let dev_list = super::PCI_DEVICES.lock();
        dev_list
            .iter()
            .filter(|dev| !dev.is_bound() && driver.id_table().iter().any(|id| id.matches(dev)))
            .cloned()
            .collect()
    };

    for dev in matching {
        ostd::info!(
            "PCI: Probing device [{}] {:04x}:{:04x} with driver \"{}\"...",
            dev.address,
            dev.vendor_id,
            dev.device_id,
            driver.name()
        );

        match driver.probe(&dev) {
            Ok(()) => {
                let mut dev_list = super::PCI_DEVICES.lock();
                if let Some(target) = dev_list.iter_mut().find(|d| d.address == dev.address) {
                    target.bound_driver = Some(driver.name());
                }
                ostd::info!(
                    "PCI: Device [{}] successfully bound to driver \"{}\"",
                    dev.address,
                    driver.name()
                );
            }
            Err(err) => {
                ostd::error!(
                    "PCI: Driver \"{}\" failed to probe device [{}]: {:?}",
                    driver.name(),
                    dev.address,
                    err
                );
            }
        }
    }

    PCI_DRIVERS.lock().push(driver);
    Ok(())
}

/// Unregisters a PCI driver and detaches it from bound devices.
pub fn pci_unregister_driver(name: &str) -> Result<()> {
    let mut drivers = PCI_DRIVERS.lock();
    if let Some(pos) = drivers.iter().position(|d| d.name() == name) {
        let driver = drivers.remove(pos);
        let mut dev_list = super::PCI_DEVICES.lock();
        for dev in dev_list.iter_mut() {
            if dev.bound_driver == Some(driver.name()) {
                let _ = driver.remove(dev);
                dev.bound_driver = None;
            }
        }
    }
    Ok(())
}
