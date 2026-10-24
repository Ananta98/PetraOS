// SPDX-License-Identifier: GPL-2.0

//! PCI Device representation and helper control methods.

use ostd::io::IoMem;

use super::{
    address::PciAddress,
    bar::PciBar,
    config::{
        PCI_CAPABILITY_POINTER, PCI_COMMAND, PCI_COMMAND_BUS_MASTER, PCI_COMMAND_MEMORY_SPACE,
        PCI_STATUS, PCI_STATUS_CAP_LIST, PCI_SUBSYSTEM_ID, PCI_SUBSYSTEM_VENDOR_ID,
        pci_read_config_u8, pci_read_config_u16, pci_read_config_u32, pci_write_config_u8,
        pci_write_config_u16, pci_write_config_u32,
    },
    msix::MsixCapability,
};
use crate::uapi::errno::{Errno, Result};

/// Represents a discovered PCI device.
#[derive(Debug, Clone)]
pub struct PciDevice {
    pub address: PciAddress,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub revision: u8,
    pub header_type: u8,
    pub bars: [Option<PciBar>; 6],
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
}

impl PciDevice {
    /// Read an 8-bit byte from this device's configuration space.
    pub fn read_config_u8(&self, offset: u8) -> u8 {
        pci_read_config_u8(self.address, offset)
    }

    /// Read a 16-bit word from this device's configuration space.
    pub fn read_config_u16(&self, offset: u8) -> u16 {
        pci_read_config_u16(self.address, offset)
    }

    /// Read a 32-bit dword from this device's configuration space.
    pub fn read_config_u32(&self, offset: u8) -> u32 {
        pci_read_config_u32(self.address, offset)
    }

    /// Write an 8-bit byte to this device's configuration space.
    pub fn write_config_u8(&self, offset: u8, val: u8) {
        pci_write_config_u8(self.address, offset, val);
    }

    /// Write a 16-bit word to this device's configuration space.
    pub fn write_config_u16(&self, offset: u8, val: u16) {
        pci_write_config_u16(self.address, offset, val);
    }

    /// Write a 32-bit dword to this device's configuration space.
    pub fn write_config_u32(&self, offset: u8, val: u32) {
        pci_write_config_u32(self.address, offset, val);
    }

    /// Enables Bus Mastering (required for DMA engines such as NVMe).
    pub fn enable_bus_mastering(&self) {
        let cmd = self.read_config_u16(PCI_COMMAND);
        self.write_config_u16(PCI_COMMAND, cmd | PCI_COMMAND_BUS_MASTER);
    }

    /// Enables Memory Mapped I/O space access.
    pub fn enable_memory_space(&self) {
        let cmd = self.read_config_u16(PCI_COMMAND);
        self.write_config_u16(PCI_COMMAND, cmd | PCI_COMMAND_MEMORY_SPACE);
    }

    /// Enables both Bus Mastering (DMA) and Memory Space decoding.
    pub fn enable_device_and_dma(&self) {
        self.enable_bus_mastering();
        self.enable_memory_space();
    }

    /// Acquires and maps the MMIO region for the given BAR index (0..5).
    pub fn map_bar(&self, index: usize) -> Result<IoMem> {
        let bar = self.bars.get(index).and_then(|b| b.as_ref()).ok_or_else(|| {
            ostd::error!("PCI: Device at {} has no valid BAR{}", self.address, index);
            Errno::ENODEV
        })?;
        let start = bar.address as usize;
        let end = start.checked_add(bar.size as usize).ok_or(Errno::EINVAL)?;
        IoMem::acquire(start..end).map_err(|_| {
            ostd::error!(
                "PCI: Failed to acquire MMIO memory region for BAR{} at {:#x}..{:#x}",
                index,
                start,
                end
            );
            Errno::EIO
        })
    }

    /// Finds a capability by ID in the PCI configuration space linked list.
    pub fn find_capability(&self, target_cap_id: u8) -> Option<u8> {
        let status = self.read_config_u16(PCI_STATUS);
        if (status & PCI_STATUS_CAP_LIST) == 0 {
            return None;
        }

        let mut cap_ptr = self.read_config_u8(PCI_CAPABILITY_POINTER);
        let mut rounds = 0;
        while cap_ptr != 0 && rounds < 64 {
            let cap_id = self.read_config_u8(cap_ptr);
            if cap_id == target_cap_id {
                return Some(cap_ptr);
            }
            cap_ptr = self.read_config_u8(cap_ptr + 1);
            rounds += 1;
        }
        None
    }

    /// Probes and parses the PCI MSI-X capability if present on the device.
    pub fn msix_capability(&self) -> Option<MsixCapability> {
        use super::config::PCI_CAP_ID_MSIX;
        let cap_offset = self.find_capability(PCI_CAP_ID_MSIX)?;
        Some(MsixCapability::parse(self, cap_offset))
    }

    /// Reads the PCI Subsystem Vendor ID (`0x2C`).
    pub fn subsystem_vendor_id(&self) -> u16 {
        self.read_config_u16(PCI_SUBSYSTEM_VENDOR_ID)
    }

    /// Reads the PCI Subsystem ID (`0x2E`).
    pub fn subsystem_id(&self) -> u16 {
        self.read_config_u16(PCI_SUBSYSTEM_ID)
    }
}
