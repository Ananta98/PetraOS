// SPDX-License-Identifier: GPL-2.0

//! Linux PCI Device & Driver Model (`<linux/pci.h>`).
//!
//! Provides Linux-compatible PCI driver registration, device matching, and BAR accessors
//! on top of PetraOS's PCI bus.

use alloc::{sync::Arc, vec::Vec};
use spin::RwLock;

use crate::drivers::bus::pci::{
    PciDevice as KernelPciDevice, PCI_ANY_ID,
};
use crate::kpi::device::base::Device;
use crate::kpi::hw::IoRegion;
use crate::uapi::errno::{Errno, Result};

/// PCI device ID matching specification, mirroring Linux `struct pci_device_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciDeviceId {
    pub vendor: u16,
    pub device: u16,
    pub class_code: u16,
    pub subclass: u16,
    pub prog_if: u16,
}

impl PciDeviceId {
    pub const ANY_ID: u16 = PCI_ANY_ID;

    /// Matches a specific device by exact vendor and device ID.
    pub const fn match_device(vendor: u16, device: u16) -> Self {
        Self {
            vendor,
            device,
            class_code: PCI_ANY_ID,
            subclass: PCI_ANY_ID,
            prog_if: PCI_ANY_ID,
        }
    }

    /// Matches any device by class, subclass, and programming interface.
    pub const fn match_class(class_code: u8, subclass: u8, prog_if: u8) -> Self {
        Self {
            vendor: PCI_ANY_ID,
            device: PCI_ANY_ID,
            class_code: class_code as u16,
            subclass: subclass as u16,
            prog_if: prog_if as u16,
        }
    }

    /// Tests whether a kernel PCI device matches this device ID pattern.
    pub fn matches(&self, dev: &KernelPciDevice) -> bool {
        let v_match = self.vendor == PCI_ANY_ID || self.vendor == dev.vendor_id;
        let d_match = self.device == PCI_ANY_ID || self.device == dev.device_id;
        let c_match = self.class_code == PCI_ANY_ID || self.class_code as u8 == dev.class_code;
        let s_match = self.subclass == PCI_ANY_ID || self.subclass as u8 == dev.subclass;
        let p_match = self.prog_if == PCI_ANY_ID || self.prog_if as u8 == dev.prog_if;
        v_match && d_match && c_match && s_match && p_match
    }
}

/// Linux `struct pci_dev`: PCI device representation in KPI.
pub struct PciDevice {
    core_device: Arc<Device>,
    kernel_pci: Arc<KernelPciDevice>,
}

impl PciDevice {
    /// Wraps a kernel PCI device into a KPI [`PciDevice`].
    pub fn new(kernel_pci: Arc<KernelPciDevice>) -> Self {
        let name = alloc::format!("pci_{:04x}:{:04x}", kernel_pci.vendor_id, kernel_pci.device_id);
        Self {
            core_device: Arc::new(Device::new(name)),
            kernel_pci,
        }
    }

    /// Returns the core device model representation.
    pub fn core_device(&self) -> &Arc<Device> {
        &self.core_device
    }

    /// Returns the PCI Vendor ID (`pci_dev->vendor`).
    pub fn vendor(&self) -> u16 {
        self.kernel_pci.vendor_id
    }

    /// Returns the PCI Device ID (`pci_dev->device`).
    pub fn device(&self) -> u16 {
        self.kernel_pci.device_id
    }

    /// Returns the PCI Interrupt Line (`pci_dev->irq`).
    pub fn irq(&self) -> u8 {
        self.kernel_pci.interrupt_line
    }

    /// Linux `pci_enable_device`: enables bus mastering and memory space decoding.
    pub fn enable_device(&self) {
        self.kernel_pci.enable_device_and_dma();
    }

    /// Linux `pci_set_master`: enables bus mastering for DMA.
    pub fn set_master(&self) {
        self.kernel_pci.enable_bus_mastering();
    }

    /// Linux `pci_iomap`: maps the given BAR into an [`IoRegion`].
    pub fn iomap_bar(&self, bar_index: usize) -> Result<IoRegion> {
        let bar = self
            .kernel_pci
            .bars
            .get(bar_index)
            .and_then(|b| b.as_ref())
            .ok_or(Errno::ENODEV)?;
        IoRegion::ioremap(bar.address as usize, bar.size as usize)
    }

    /// Read an 8-bit config register (`pci_read_config_byte`).
    pub fn read_config_u8(&self, offset: u8) -> u8 {
        self.kernel_pci.read_config_u8(offset)
    }

    /// Read a 16-bit config register (`pci_read_config_word`).
    pub fn read_config_u16(&self, offset: u8) -> u16 {
        self.kernel_pci.read_config_u16(offset)
    }

    /// Read a 32-bit config register (`pci_read_config_dword`).
    pub fn read_config_u32(&self, offset: u8) -> u32 {
        self.kernel_pci.read_config_u32(offset)
    }

    /// Write an 8-bit config register (`pci_write_config_byte`).
    pub fn write_config_u8(&self, offset: u8, val: u8) {
        self.kernel_pci.write_config_u8(offset, val);
    }

    /// Write a 16-bit config register (`pci_write_config_word`).
    pub fn write_config_u16(&self, offset: u8, val: u16) {
        self.kernel_pci.write_config_u16(offset, val);
    }

    /// Write a 32-bit config register (`pci_write_config_dword`).
    pub fn write_config_u32(&self, offset: u8, val: u32) {
        self.kernel_pci.write_config_u32(offset, val);
    }

    /// Reference to the underlying kernel PCI device.
    pub fn kernel_device(&self) -> &Arc<KernelPciDevice> {
        &self.kernel_pci
    }
}

/// Linux `struct pci_driver`: driver abstraction for PCI devices.
pub trait PciDriver: Send + Sync + 'static {
    /// Name of the PCI driver.
    fn name(&self) -> &str;

    /// Table of supported PCI devices.
    fn id_table(&self) -> &'static [PciDeviceId];

    /// Probe callback invoked when a matching device is discovered.
    fn probe(&self, pdev: &Arc<PciDevice>) -> Result<()>;

    /// Remove callback invoked when the device is unbound.
    fn remove(&self, pdev: &Arc<PciDevice>) -> Result<()>;
}

/// Registry of registered KPI PCI drivers.
pub struct PciDriverRegistry {
    drivers: RwLock<Vec<Arc<dyn PciDriver>>>,
}

impl PciDriverRegistry {
    /// Creates a new driver registry.
    pub const fn new() -> Self {
        Self {
            drivers: RwLock::new(Vec::new()),
        }
    }

    /// Registers a PCI driver.
    pub fn register_driver(&self, driver: Arc<dyn PciDriver>) -> Result<()> {
        let mut guard = self.drivers.write();
        if guard.iter().any(|d| d.name() == driver.name()) {
            crate::return_errno!(EEXIST, "pci driver already registered");
        }
        guard.push(driver);
        Ok(())
    }

    /// Unregisters a PCI driver by name.
    pub fn unregister_driver(&self, driver_name: &str) -> Result<()> {
        let mut guard = self.drivers.write();
        let index = guard
            .iter()
            .position(|d| d.name() == driver_name)
            .ok_or(Errno::ENODEV)?;
        guard.remove(index);
        Ok(())
    }
}

static PCI_REGISTRY: PciDriverRegistry = PciDriverRegistry::new();

/// Linux `pci_register_driver`: registers a PCI driver into the KPI system.
pub fn pci_register_driver(driver: Arc<dyn PciDriver>) -> Result<()> {
    PCI_REGISTRY.register_driver(driver)
}

/// Linux `pci_unregister_driver`: unregisters a PCI driver from the KPI system.
pub fn pci_unregister_driver(driver_name: &str) -> Result<()> {
    PCI_REGISTRY.unregister_driver(driver_name)
}
