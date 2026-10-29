// SPDX-License-Identifier: GPL-2.0

//! PCI MSI-X (Message Signaled Interrupts Extended) Capability.
//!
//! Provides parsing and configuration methods for the standard PCI MSI-X capability
//! (PCI-SIG Local Bus Specification 3.0).

use super::device::PciDevice;

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
}
