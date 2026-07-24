//! MSI and MSI-X interrupt support for PCI devices on x86_64 architecture.
//!
//! On x86_64 with Local APIC:
//! - An MSI write targets physical address `0xFEE0_0000 | (APIC_ID << 12)`.
//! - The message data encodes the assigned interrupt vector (range 32..255).

use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::Error;

use crate::drivers::pci::capability::{CAP_MSI, CAP_MSIX, capabilities};
use crate::drivers::pci::device::PciDevice;
use crate::irq::{IrqHandler, IrqRegistration};


/// The base Local APIC message address for x86_64 MSI.
pub const MSI_ADDR_BASE: u32 = 0xFEE0_0000;

/// Construct an x86_64 MSI Message Address targeting a specific CPU APIC ID.
///
/// Address format:
/// `[31:20]` = 0xFEE
/// `[19:12]` = Destination APIC ID
/// `[11:4]`  = Reserved (0)
/// `[3]`     = Redirection Hint (0 = physical)
/// `[2]`     = Destination Mode (0 = physical)
/// `[1:0]`   = Reserved (0)
pub fn msi_address(apic_id: u8) -> u32 {
    MSI_ADDR_BASE | ((apic_id as u32) << 12)
}

/// Construct an x86_64 MSI Message Data value for a given APIC interrupt vector.
///
/// Data format:
/// `[15:11]` = Reserved (0)
/// `[10:8]`  = Delivery Mode (000 = Fixed)
/// `[7:0]`   = Interrupt Vector (32..255)
pub fn msi_data(vector: u8) -> u16 {
    vector as u16
}

/// The number of MSI vectors requested by a PCI device.
///
/// Must be a power of two (1, 2, 4, 8, 16, or 32).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MsiVectorCount(u16);

impl MsiVectorCount {
    /// Create a new `MsiVectorCount` if `n` is a power of two <= 32.
    pub fn new(n: u16) -> Option<Self> {
        if n.is_power_of_two() && n <= 32 {
            Some(Self(n))
        } else {
            None
        }
    }

    /// Return the count as a `u16`.
    pub fn get(self) -> u16 {
        self.0
    }

    /// Return the log2 encoding (0..=5) for the PCI Multiple Message Enable (MME) field.
    pub fn log2(self) -> u8 {
        match self.0 {
            1 => 0,
            2 => 1,
            4 => 2,
            8 => 3,
            16 => 4,
            32 => 5,
            _ => 0,
        }
    }
}

/// Descriptor for a configured MSI interrupt on x86_64.
pub struct MsiConfig {
    /// Allocated IRQ registrations, one per vector.
    pub vectors: Vec<IrqRegistration>,
    /// The Message Address value programmed into the device.
    pub message_address: u32,
    /// The Message Data value programmed into the device.
    pub message_data: u16,
    /// Number of allocated vectors.
    pub count: MsiVectorCount,
    /// Configuration space offset of the MSI capability.
    pub cap_offset: u8,
}

/// Descriptor for a configured MSI-X interrupt setup on x86_64.
pub struct MsixConfig {
    /// Allocated IRQ registrations, one per vector.
    pub vectors: Vec<IrqRegistration>,
    /// Per-vector message address/data pairs.
    pub entries: Vec<MsixEntry>,
    /// Configuration space offset of the MSI-X capability.
    pub cap_offset: u8,
}

/// A single MSI-X table entry descriptor.
#[derive(Debug, Clone, Copy)]
pub struct MsixEntry {
    /// Vector index in the MSI-X table.
    pub entry_index: u16,
    /// Message Address programmed into the MSI-X table.
    pub message_address: u64,
    /// Message Data programmed into the MSI-X table.
    pub message_data: u32,
    /// Vector control bits (Bit 0 = Masked).
    pub vector_control: u32,
}

/// Find the MSI capability offset in configuration space for `device`.
pub fn find_msi_capability(device: &PciDevice) -> Option<u8> {
    capabilities(device)
        .into_iter()
        .find(|cap| cap.id == CAP_MSI)
        .map(|cap| cap.offset)
}

/// Find the MSI-X capability offset in configuration space for `device`.
pub fn find_msix_capability(device: &PciDevice) -> Option<u8> {
    capabilities(device)
        .into_iter()
        .find(|cap| cap.id == CAP_MSIX)
        .map(|cap| cap.offset)
}

/// Enable single-vector MSI for a PCI device on x86_64.
///
/// Allocates an IRQ line, configures x86_64 Local APIC message address/data,
/// programs the device's MSI capability registers, and enables MSI.
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

    // Set MSI Enable (Bit 0 = 1) and set MME (Bits 6:4 = 0 for 1 vector)
    let new_msg_ctrl = (msg_ctrl & !(0b111 << 4)) | (1 << 0);
    device.write_config_u16(cap_offset + 2, new_msg_ctrl);

    Ok(MsiConfig {
        vectors: alloc::vec![irq_reg],
        message_address: msg_addr,
        message_data: msg_data,
        count: MsiVectorCount::new(1).unwrap(),
        cap_offset,
    })
}

/// Enable multi-vector MSI for a PCI device on x86_64.
pub fn enable_msi_vectors(
    device: &PciDevice,
    count: MsiVectorCount,
    handlers: Vec<Arc<dyn IrqHandler>>,
) -> Result<MsiConfig, Error> {
    let num_vectors = count.get() as usize;
    if handlers.len() != num_vectors {
        return Err(Error::InvalidArgs);
    }


    let cap_offset = find_msi_capability(device).ok_or(Error::NotEnoughResources)?;
    let msg_ctrl = device.read_config_u16(cap_offset + 2);

    let mmc = (msg_ctrl >> 1) & 0x7;
    let max_capable = 1u16 << mmc;
    if count.get() > max_capable {
        return Err(Error::NotEnoughResources);
    }

    let mut registrations = Vec::with_capacity(num_vectors);
    for h in handlers {
        registrations.push(IrqRegistration::alloc_any(h)?);
    }

    let base_vector = registrations[0].num();
    let msg_addr = msi_address(0);
    let msg_data = msi_data(base_vector);
    let is_64bit = (msg_ctrl & (1 << 7)) != 0;

    device.write_config_u32(cap_offset + 4, msg_addr);

    if is_64bit {
        device.write_config_u32(cap_offset + 8, 0);
        device.write_config_u16(cap_offset + 12, msg_data);
    } else {
        device.write_config_u16(cap_offset + 8, msg_data);
    }

    let mme = count.log2();
    let new_msg_ctrl = (msg_ctrl & !(0b111 << 4)) | ((mme as u16) << 4) | (1 << 0);
    device.write_config_u16(cap_offset + 2, new_msg_ctrl);

    Ok(MsiConfig {
        vectors: registrations,
        message_address: msg_addr,
        message_data: msg_data,
        count,
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

/// Enable MSI-X capability for a PCI device on x86_64.
///
/// Sets Bit 15 (MSI-X Enable) and clears Bit 14 (Function Mask) in the capability control register.
pub fn enable_msix(device: &PciDevice) -> Result<u8, Error> {
    let cap_offset = find_msix_capability(device).ok_or(Error::NotEnoughResources)?;
    let msg_ctrl = device.read_config_u16(cap_offset + 2);
    let new_ctrl = (msg_ctrl | (1 << 15)) & !(1 << 14);
    device.write_config_u16(cap_offset + 2, new_ctrl);
    Ok(cap_offset)
}

/// Disable MSI-X for a PCI device.
pub fn disable_msix(device: &PciDevice) {
    if let Some(cap_offset) = find_msix_capability(device) {
        let msg_ctrl = device.read_config_u16(cap_offset + 2);
        device.write_config_u16(cap_offset + 2, msg_ctrl & !(1 << 15));
    }
}

/// Mask or unmask an MSI vector (if Per-Vector Masking is supported by the capability).
pub fn mask_msi_vector(device: &PciDevice, vector_idx: u8, masked: bool) {
    if let Some(cap_offset) = find_msi_capability(device) {
        let msg_ctrl = device.read_config_u16(cap_offset + 2);
        let is_64bit = (msg_ctrl & (1 << 7)) != 0;
        let pvm = (msg_ctrl & (1 << 8)) != 0;
        if !pvm || vector_idx >= 32 {
            return;
        }

        let mask_reg_offset = if is_64bit {
            cap_offset + 16
        } else {
            cap_offset + 12
        };
        let mut mask_bits = device.read_config_u32(mask_reg_offset);
        if masked {
            mask_bits |= 1 << vector_idx;
        } else {
            mask_bits &= !(1 << vector_idx);
        }
        device.write_config_u32(mask_reg_offset, mask_bits);
    }
}
