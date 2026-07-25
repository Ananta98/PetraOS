//! MSI interrupt support for PCI devices on x86_64 architecture.
//!
//! On x86_64 with Local APIC:
//! - An MSI write targets physical address `0xFEE0_0000 | (APIC_ID << 12)`.
//! - The message data encodes the assigned interrupt vector (range 32..255).

use alloc::vec::Vec;
use ostd::Error;

use crate::drivers::pci::capability::{CAP_MSI, capabilities};
use crate::drivers::pci::device::PciDevice;
use crate::irq::{IrqHandler, IrqRegistration};

/// The base Local APIC message address for x86_64 MSI.
pub const MSI_ADDR_BASE: u32 = 0xFEE0_0000;

/// Construct an x86_64 MSI Message Address targeting a specific CPU APIC ID.
pub fn msi_address(apic_id: u8) -> u32 {
    MSI_ADDR_BASE | ((apic_id as u32) << 12)
}

/// Construct an x86_64 MSI Message Data value for a given APIC interrupt vector.
pub fn msi_data(vector: u8) -> u16 {
    vector as u16
}

/// Descriptor for a configured MSI interrupt on x86_64.
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

/// Enable single-vector MSI for a PCI device on x86_64.
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

