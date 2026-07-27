//! Vendor-neutral Message Signaled Interrupts (MSI) support for PCI devices.
//!
//! Provides capability discovery, register programming (32-bit and 64-bit addresses),
//! and interrupt enablement/disablement in PCI configuration space per the PCI Local Bus Specification.

use alloc::vec::Vec;
use ostd::Error;

use super::arch::{self, msi_address, msi_data};
use super::capability::{CAP_MSI, capabilities};
use super::device::PciDevice;
use crate::irq::{IrqHandler, IrqRegistration};

/// Descriptor for a configured MSI interrupt.
pub struct MsiConfig {
    /// Allocated IRQ registration.
    pub vectors: Vec<IrqRegistration>,
    /// The Message Address value programmed into the device.
    pub message_address: u32,
    /// The Message Data value programmed into the device.
    pub message_data: u16,
    /// Configuration space offset of the MSI capability.
    pub cap_offset: u8,
}

/// Find the MSI capability offset in configuration space for `device`.
pub fn find_msi_capability(device: &PciDevice) -> Option<u8> {
    capabilities(device)
        .into_iter()
        .find(|cap| cap.id == CAP_MSI)
        .map(|cap| cap.offset)
}

/// Enable single-vector MSI for a PCI device.
pub fn enable_msi(device: &PciDevice, handler: impl IrqHandler) -> Result<MsiConfig, Error> {
    let cap_offset = find_msi_capability(device).ok_or(Error::NotEnoughResources)?;

    let irq_reg = IrqRegistration::alloc_any(handler)?;
    let vector = irq_reg.num();

    let msg_addr = msi_address(0);
    let msg_data = msi_data(vector);

    let msg_ctrl = device.read_config_u16(cap_offset + 2);
    let is_64bit = (msg_ctrl & (1 << 7)) != 0;

    device.write_config_u32(cap_offset + 4, msg_addr);

    if is_64bit {
        device.write_config_u32(cap_offset + 8, 0);
        device.write_config_u16(cap_offset + 12, msg_data);
    } else {
        device.write_config_u16(cap_offset + 8, msg_data);
    }

    // Set MSI Enable (Bit 0 = 1) and clear MME (Bits 6:4 = 0)
    let new_msg_ctrl = (msg_ctrl & !(0b111 << 4)) | (1 << 0);
    device.write_config_u16(cap_offset + 2, new_msg_ctrl);

    Ok(MsiConfig {
        vectors: alloc::vec![irq_reg],
        message_address: msg_addr,
        message_data: msg_data,
        cap_offset,
    })
}

/// Disable MSI for a PCI device.
pub fn disable_msi(device: &PciDevice) {
    if let Some(cap_offset) = find_msi_capability(device) {
        let msg_ctrl = device.read_config_u16(cap_offset + 2);
        device.write_config_u16(cap_offset + 2, msg_ctrl & !(1 << 0));
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_msi_address_and_data() {
        assert_eq!(msi_address(0), 0xFEE0_0000);
        assert_eq!(msi_address(1), 0xFEE0_1000);
        assert_eq!(msi_address(4), 0xFEE0_4000);

        assert_eq!(msi_data(32), 32);
        assert_eq!(msi_data(255), 255);
    }
}
