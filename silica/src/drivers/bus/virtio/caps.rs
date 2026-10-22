// SPDX-License-Identifier: GPL-2.0

//! Discovery of the vendor-specific PCI capabilities a modern virtio device
//! publishes (`struct virtio_pci_cap`).

use crate::drivers::bus::pci::{BarType, PciBar, PciDevice};
use crate::uapi::errno::{Errno, Result};

/// `virtio_pci_cap::cfg_type` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtioCapType {
    /// `VIRTIO_PCI_CAP_COMMON_CFG`: the `virtio_pci_common_cfg` register set.
    Common,
    /// `VIRTIO_PCI_CAP_NOTIFY_CFG`: queue notification registers.
    Notify,
    /// `VIRTIO_PCI_CAP_ISR_CFG`: legacy-style interrupt cause byte.
    Isr,
    /// `VIRTIO_PCI_CAP_DEVICE_CFG`: the device's own config space.
    Device,
}

/// One `struct virtio_pci_cap` (plus the notify multiplier when present).
#[derive(Debug, Clone, Copy)]
pub struct VirtioCap {
    pub kind: VirtioCapType,
    pub bar: u8,
    pub offset: u32,
    pub length: u32,
    /// `virtio_pci_notify_cap::notify_off_multiplier`.
    pub notify_multiplier: u32,
}

impl VirtioCap {
    fn kind_of(cfg_type: u8) -> Option<VirtioCapType> {
        match cfg_type {
            0 => None, // virtio_pci_capabilities_cap, ignored here
            1 => Some(VirtioCapType::Common),
            2 => Some(VirtioCapType::Notify),
            3 => Some(VirtioCapType::Isr),
            4 => Some(VirtioCapType::Device),
            _ => None,
        }
    }

    fn parse(device: &PciDevice, cap_base: u8, cap_len: u8) -> Option<VirtioCap> {
        let kind = Self::kind_of(device.read_config_u8(cap_base + 0x03))?;
        let bar = device.read_config_u8(cap_base + 0x04);
        let offset = device.read_config_u32(cap_base + 0x08);
        let length = device.read_config_u32(cap_base + 0x0C);
        // The notify cap appends `__u32 notify_off_multiplier`.
        let notify_multiplier = if cap_len > 0x10 && kind == VirtioCapType::Notify {
            device.read_config_u32(cap_base + 0x10)
        } else {
            0
        };
        Some(VirtioCap {
            kind,
            bar,
            offset,
            length,
            notify_multiplier,
        })
    }
}

/// The necessary capabilities of one virtio PCI device.
#[derive(Debug, Clone, Copy)]
pub struct VirtioCaps {
    pub common: Option<VirtioCap>,
    pub notify: Option<VirtioCap>,
    pub isr: Option<VirtioCap>,
    pub device: Option<VirtioCap>,
}

/// Walks the PCI capability list looking for the virtio vendor caps
/// (`cap_vndr = 0x09`).
pub fn find_caps(device: &PciDevice) -> Result<VirtioCaps> {
    let mut caps = VirtioCaps {
        common: None,
        notify: None,
        isr: None,
        device: None,
    };
    if device.read_config_u8(0x34) == 0 {
        crate::return_errno!(ENODEV, "device exposes no capability list");
    }
    let mut cursor = device.read_config_u8(0x34);
    let mut rounds = 0;
    while cursor != 0 && rounds < 64 {
        rounds += 1;
        let cap_id = device.read_config_u8(cursor);
        let next = device.read_config_u8(cursor + 1);
        let cap_len = device.read_config_u8(cursor + 2);
        if cap_id == 0x09
            && let Some(cap) = VirtioCap::parse(device, cursor, cap_len)
        {
            match cap.kind {
                VirtioCapType::Common => {
                    caps.common = Some(cap);
                }
                VirtioCapType::Notify => {
                    caps.notify = Some(cap);
                }
                VirtioCapType::Isr => {
                    caps.isr = Some(cap);
                }
                VirtioCapType::Device => {
                    caps.device = Some(cap);
                }
            }
        }
        cursor = next;
    }

    let caps_ok = caps.common.is_some() && caps.notify.is_some();
    if caps_ok {
        Ok(caps)
    } else {
        crate::return_errno!(ENODEV, "missing common or notify capability");
    }
}

/// Maps one capability into kernel virtual memory.
///
/// `cap.bar` selects the PCI BAR and the offset/length pick the window.
pub fn map_cap(device: &PciDevice, cap: &VirtioCap) -> Result<ostd::io::IoMem> {
    let bar: PciBar = device
        .bars
        .get(cap.bar as usize)
        .and_then(|bar| *bar)
        .ok_or(Errno::ENODEV)?;
    if !matches!(bar.bar_type, BarType::Memory32 | BarType::Memory64) {
        crate::return_errno!(EIO, "virtio capability is not memory mapped");
    }
    let start = bar.address as usize + cap.offset as usize;
    let len = cap.length as usize;
    ostd::io::IoMem::acquire(start..start + len).map_err(|_| Errno::EIO)
}
