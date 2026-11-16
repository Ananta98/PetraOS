// SPDX-License-Identifier: GPL-2.0

//! PCI MSI (Message Signaled Interrupts) Capability.
//!
//! Configures single-vector MSI on x86_64 (`0xFEE0_0000` LAPIC target).

use super::device::PciDevice;

const MSI_CTRL_ENABLE: u16 = 1 << 0;
const MSI_CTRL_64BIT: u16 = 1 << 7;
const MSI_CTRL_MME_MASK: u16 = 0b111 << 4;
const X86_LAPIC_MSI_ADDR: u32 = 0xFEE0_0000;

/// Parsed PCI MSI capability handle.
#[derive(Debug, Clone, Copy)]
pub struct MsiCapability {
    pub cap_offset: u8,
    pub is_64bit: bool,
}

impl MsiCapability {
    /// Parses an MSI capability at `cap_offset`.
    pub fn parse(dev: &PciDevice, cap_offset: u8) -> Self {
        let ctrl = dev.read_config_u16(cap_offset + 0x02);
        Self {
            cap_offset,
            is_64bit: (ctrl & MSI_CTRL_64BIT) != 0,
        }
    }

    /// Programs a single MSI vector targeting BSP LAPIC (`0xFEE0_0000`) with IDT `vector`
    /// and enables MSI.
    pub fn enable_vector(&self, dev: &PciDevice, vector: u8) {
        dev.write_config_u32(self.cap_offset + 0x04, X86_LAPIC_MSI_ADDR);
        if self.is_64bit {
            dev.write_config_u32(self.cap_offset + 0x08, 0);
            dev.write_config_u16(self.cap_offset + 0x0C, vector as u16);
        } else {
            dev.write_config_u16(self.cap_offset + 0x08, vector as u16);
        }

        let mut ctrl = dev.read_config_u16(self.cap_offset + 0x02);
        ctrl &= !MSI_CTRL_MME_MASK;
        ctrl |= MSI_CTRL_ENABLE;
        dev.write_config_u16(self.cap_offset + 0x02, ctrl);
    }
}
