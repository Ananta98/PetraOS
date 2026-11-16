// SPDX-License-Identifier: GPL-2.0

//! PCI MSI-X (Message Signaled Interrupts Extended) Capability.
//!
//! Provides parsing and configuration methods for the standard PCI MSI-X capability
//! (PCI-SIG Local Bus Specification 3.0).

use ostd::io::IoMem;
use ostd::mm::VmIoOnce;

use super::bar::BarType;
use super::device::PciDevice;
use crate::uapi::errno::{Errno, Result};

/// PCI MSI-X Capability structure.
#[derive(Debug, Clone, Copy)]
pub struct MsixCapability {
    /// Configuration space offset of this capability.
    pub cap_offset: u8,
    /// Encoded table size (0-based, actual size is table_size + 1).
    pub table_size: u16,
    /// BAR Indicator Register (BIR) for MSI-X table (0..=5).
    pub table_bir: u8,
    /// Offset within the specified BAR for the MSI-X table.
    pub table_offset: u32,
    /// BAR Indicator Register (BIR) for Pending Bit Array (PBA) (0..=5).
    pub pba_bir: u8,
    /// Offset within the specified BAR for the PBA.
    pub pba_offset: u32,
}

impl MsixCapability {
    /// Attempts to parse an MSI-X capability structure starting at `cap_offset`.
    pub fn parse(dev: &PciDevice, cap_offset: u8) -> Self {
        // Offset 0x02: Message Control (16-bit)
        let ctrl = dev.read_config_u16(cap_offset + 0x02);
        let table_size = ctrl & 0x07FF; // Bits 10:0

        // Offset 0x04: Table Offset & BIR (32-bit)
        let table_raw = dev.read_config_u32(cap_offset + 0x04);
        let table_bir = (table_raw & 0x07) as u8;
        let table_offset = table_raw & !0x07;

        // Offset 0x08: PBA Offset & BIR (32-bit)
        let pba_raw = dev.read_config_u32(cap_offset + 0x08);
        let pba_bir = (pba_raw & 0x07) as u8;
        let pba_offset = pba_raw & !0x07;

        Self {
            cap_offset,
            table_size,
            table_bir,
            table_offset,
            pba_bir,
            pba_offset,
        }
    }

    /// Number of interrupt vectors supported (table_size + 1).
    pub fn vector_count(&self) -> u16 {
        self.table_size + 1
    }

    /// Checks if MSI-X is enabled (bit 15 of Message Control).
    pub fn is_enabled(&self, dev: &PciDevice) -> bool {
        let ctrl = dev.read_config_u16(self.cap_offset + 0x02);
        (ctrl & (1 << 15)) != 0
    }

    /// Enables or disables MSI-X.
    pub fn set_enabled(&self, dev: &PciDevice, enable: bool) {
        let mut ctrl = dev.read_config_u16(self.cap_offset + 0x02);
        if enable {
            ctrl |= 1 << 15;
        } else {
            ctrl &= !(1 << 15);
        }
        dev.write_config_u16(self.cap_offset + 0x02, ctrl);
    }

    /// Sets the Function Mask (bit 14 of Message Control).
    /// When masked, all MSI-X vectors are masked regardless of vector table mask bits.
    pub fn set_function_mask(&self, dev: &PciDevice, mask: bool) {
        let mut ctrl = dev.read_config_u16(self.cap_offset + 0x02);
        if mask {
            ctrl |= 1 << 14;
        } else {
            ctrl &= !(1 << 14);
        }
        dev.write_config_u16(self.cap_offset + 0x02, ctrl);
    }

    /// Maps the MSI-X table MMIO region for this capability.
    pub fn map_table(&self, dev: &PciDevice) -> Result<IoMem> {
        let bar = dev
            .bars
            .get(self.table_bir as usize)
            .and_then(|b| *b)
            .ok_or(Errno::ENODEV)?;
        if !matches!(bar.bar_type, BarType::Memory32 | BarType::Memory64) {
            return Err(Errno::EIO);
        }
        let start = bar.address as usize + self.table_offset as usize;
        let len = (self.vector_count() as usize) * 16;
        IoMem::acquire(start..start + len).map_err(|_| Errno::EIO)
    }

    /// Programs and unmasks MSI-X table entry `index` to deliver IDT `vector` on BSP LAPIC.
    pub fn program_vector(&self, table: &IoMem, index: u16, vector: u8) -> Result<()> {
        if index >= self.vector_count() {
            return Err(Errno::EINVAL);
        }
        let base = (index as usize) * 16;
        table.write_once(base + 0x00, &0xFEE0_0000u32).map_err(|_| Errno::EIO)?;
        table.write_once(base + 0x04, &0u32).map_err(|_| Errno::EIO)?;
        table.write_once(base + 0x08, &(vector as u32)).map_err(|_| Errno::EIO)?;
        table.write_once(base + 0x0C, &0u32).map_err(|_| Errno::EIO)?;
        Ok(())
    }
}
