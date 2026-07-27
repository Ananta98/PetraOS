use super::device::PciDevice;
/// PCI Capability List Walking
///
/// PCI devices can advertise optional capabilities through a linked list
/// in configuration space. Each capability has an ID byte and a pointer
/// to the next capability.
use alloc::vec::Vec;

// Well-known PCI capability IDs
/// Power Management capability
pub const CAP_PM: u8 = 0x01;
/// MSI (Message Signaled Interrupts) capability
pub const CAP_MSI: u8 = 0x05;
/// Vendor-specific capability
pub const CAP_VENDOR: u8 = 0x09;
/// PCI Express capability
pub const CAP_PCIE: u8 = 0x10;
/// MSI-X capability
pub const CAP_MSIX: u8 = 0x11;

/// A single entry in the PCI capability linked list.
#[derive(Debug, Clone, Copy)]
pub struct PciCapability {
    /// Capability ID (identifies the type of capability).
    pub id: u8,
    /// Offset of this capability in configuration space.
    pub offset: u8,
}

/// Walk the PCI capability linked list for a device.
///
/// Returns all capabilities found. The capability list is present only
/// if bit 4 of the status register is set. Each entry in the list
/// contains an ID byte at offset+0 and a next-pointer at offset+1.
pub fn capabilities(device: &PciDevice) -> Vec<PciCapability> {
    let mut result = Vec::new();

    // Check if capabilities list is supported (bit 4 of status register)
    let status = device.status();
    if (status & (1 << 4)) == 0 {
        return result;
    }

    // Read capabilities pointer, aligned to dword boundary
    let mut ptr = device.capabilities_ptr();

    // Walk the linked list (with a safety limit to avoid infinite loops)
    let mut visited = 0u32;
    while ptr != 0 && visited < 48 {
        let cap_id = device.read_config_u8(ptr);
        let next_ptr = device.read_config_u8(ptr + 1) & 0xFC;

        result.push(PciCapability {
            id: cap_id,
            offset: ptr,
        });

        ptr = next_ptr;
        visited += 1;
    }

    result
}
