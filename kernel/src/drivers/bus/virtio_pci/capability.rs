/// VirtIO PCI Capability Parsing
///
/// VirtIO modern devices expose their configuration regions (common config,
/// notification, ISR, and device-specific config) via PCI vendor-specific
/// capabilities (ID 0x09). Each capability points to a specific BAR region
/// through a type field, BAR index, offset, and length.
///
/// This module parses the PCI capability linked list to locate all VirtIO
/// capabilities and surfaces them for use by the transport layer.
///
/// References:
/// - VirtIO 1.2 Specification §4.1.4.1 (VirtIO Structure PCI Capabilities)
use super::regs;
use crate::drivers::bus::pci::{CAP_VENDOR, PciDevice, capabilities};
use alloc::vec::Vec;

/// A parsed VirtIO-specific PCI vendor capability.
///
/// Each capability identifies a memory region within a particular BAR that
/// serves a specific configuration role (common config, notifications, ISR,
/// or device-specific config).
#[derive(Debug, Clone, Copy)]
pub struct VirtioPciCapability {
    /// VirtIO capability type (`CAP_TYPE_*` constant from `regs`).
    pub cap_type: u8,
    /// BAR index (0–5) that contains the described region.
    pub bar: u8,
    /// Byte offset of the region within the BAR.
    pub offset: u32,
    /// Length of the region in bytes.
    pub length: u32,
    /// Extra 4-byte field — for `CAP_TYPE_NOTIFY_CFG` this holds the
    /// `notify_off_multiplier`. Zero for all other capability types.
    pub extra: u32,
}

/// Parse all VirtIO-specific PCI capabilities from a PCI device.
///
/// Walks the standard PCI capability linked list. For each vendor-specific
/// capability (`CAP_VENDOR`, ID 0x09), reads the VirtIO cap-type byte and
/// additional fields from configuration space.
///
/// Returns only entries whose length is non-zero and whose cap-type is a
/// known VirtIO type; unknown/malformed entries are silently skipped.
pub fn parse_virtio_capabilities(device: &PciDevice) -> Vec<VirtioPciCapability> {
    let pci_caps = capabilities(device);
    let mut virtio_caps = Vec::new();

    for pci_cap in pci_caps {
        if pci_cap.id != CAP_VENDOR {
            continue;
        }

        let cap_offset = pci_cap.offset;

        // Read the VirtIO-specific cap_type byte.
        let cap_type = device.read_config_u8(cap_offset + regs::CAP_FIELD_CAP_TYPE);

        // Only admit known VirtIO capability types.
        if !is_known_cap_type(cap_type) {
            continue;
        }

        let bar = device.read_config_u8(cap_offset + regs::CAP_FIELD_BAR);
        // BAR index must be in range 0–5.
        if bar > 5 {
            continue;
        }

        let offset = device.read_config_u32(cap_offset + regs::CAP_FIELD_OFFSET);
        let length = device.read_config_u32(cap_offset + regs::CAP_FIELD_LENGTH);

        if length == 0 {
            continue;
        }

        // The extra field is present only for NOTIFY_CFG; read it for all
        // types for simplicity — callers ignore it for non-NOTIFY caps.
        let extra = device.read_config_u32(cap_offset + regs::CAP_FIELD_EXTRA);

        virtio_caps.push(VirtioPciCapability {
            cap_type,
            bar,
            offset,
            length,
            extra,
        });
    }

    virtio_caps
}

/// Find the first capability with the given VirtIO cap type.
///
/// Convenience helper for the transport layer to locate specific regions
/// (e.g., common config, notification, ISR, device config).
pub fn find_cap(caps: &[VirtioPciCapability], cap_type: u8) -> Option<&VirtioPciCapability> {
    caps.iter().find(|cap| cap.cap_type == cap_type)
}

/// Return `true` if `cap_type` is a recognized VirtIO PCI capability type.
fn is_known_cap_type(cap_type: u8) -> bool {
    matches!(
        cap_type,
        regs::CAP_TYPE_COMMON_CFG
            | regs::CAP_TYPE_NOTIFY_CFG
            | regs::CAP_TYPE_ISR_CFG
            | regs::CAP_TYPE_DEVICE_CFG
            | regs::CAP_TYPE_PCI_CFG
    )
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    /// Verify that `is_known_cap_type` accepts all defined VirtIO cap types
    /// and rejects unknown ones.
    #[ktest]
    fn test_known_cap_type_filter() {
        assert!(is_known_cap_type(regs::CAP_TYPE_COMMON_CFG));
        assert!(is_known_cap_type(regs::CAP_TYPE_NOTIFY_CFG));
        assert!(is_known_cap_type(regs::CAP_TYPE_ISR_CFG));
        assert!(is_known_cap_type(regs::CAP_TYPE_DEVICE_CFG));
        assert!(is_known_cap_type(regs::CAP_TYPE_PCI_CFG));

        // Unknown types must be rejected.
        assert!(!is_known_cap_type(0));
        assert!(!is_known_cap_type(6));
        assert!(!is_known_cap_type(0xFF));
    }
}
