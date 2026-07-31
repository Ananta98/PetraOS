/// VirtIO PCI Transport Layer
///
/// Provides [`VirtioPciTransport`], a unified abstraction over the two
/// VirtIO-over-PCI transport variants:
///
/// * **Legacy transport** — used by transitional (pre-1.0) devices, or modern
///   devices that also support the legacy interface. Configuration registers
///   are accessed via an I/O-space or memory-mapped BAR0.
///
/// * **Modern transport** — used by VirtIO 1.0+ devices. Configuration
///   regions (common config, notification, ISR, device-specific config) are
///   located through VirtIO vendor-specific PCI capabilities.
///
/// # Initialization sequence
///
/// Both transports follow the same logical sequence mandated by the VirtIO
/// specification §3.1:
///
/// 1. `reset()` — write 0 to the status register.
/// 2. Set `STATUS_ACKNOWLEDGE` — tell the device the OS has noticed it.
/// 3. Set `STATUS_DRIVER` — tell the device the OS knows how to drive it.
/// 4. `read_device_features()` — read the 64-bit feature bitmap.
/// 5. `write_driver_features()` — write the features the driver accepts.
/// 6. Set `STATUS_FEATURES_OK` — lock in feature negotiation.
/// 7. Re-read status; if `FEATURES_OK` is clear, negotiation failed.
/// 8. Configure virtqueues via `select_queue`, `read_queue_size`,
///    `write_queue_*_addr`, and `enable_queue`.
/// 9. Set `STATUS_DRIVER_OK` — device is live.
///
/// # References
/// - VirtIO 1.2 Specification §3.1 (Driver Requirements: Device Initialization)
/// - VirtIO 1.2 Specification §4.1  (Virtio Over PCI Bus)
use super::capability::{VirtioPciCapability, find_cap, parse_virtio_capabilities};
use super::regs;
use crate::drivers::bus::pci::{PciBar, PciDevice};
use ostd::io::IoMem;
use ostd::mm::VmIoOnce;

// ──────────────────────────────────────────────────────────────
// TransportKind — legacy vs. modern dispatch enum
// ──────────────────────────────────────────────────────────────

/// The active VirtIO-over-PCI transport variant selected after probing.
pub enum TransportKind {
    /// Legacy transport: all registers are within a single BAR (usually I/O space BAR0).
    Legacy {
        /// Mapped I/O region covering the legacy register layout.
        io_bar: IoMem,
    },
    /// Modern transport (VirtIO 1.0+): configuration split across multiple BAR regions.
    Modern {
        /// Common configuration BAR region.
        common_bar: IoMem,
        /// Notification BAR region.
        notify_bar: IoMem,
        /// Multiplier applied to `queue_notify_off` to get the notification offset.
        notify_offset_multiplier: u32,
        /// ISR status BAR region.
        isr_bar: IoMem,
        /// Device-specific configuration BAR region.
        device_cfg_bar: IoMem,
    },
}

// ──────────────────────────────────────────────────────────────
// VirtioPciTransport
// ──────────────────────────────────────────────────────────────

/// A VirtIO-over-PCI transport, encapsulating hardware access for any
/// VirtIO device type.
///
/// Construct via [`VirtioPciTransport::probe`], which automatically detects
/// whether the device exposes a modern or legacy transport interface.
pub struct VirtioPciTransport {
    /// The underlying PCI device for config-space operations.
    pub pci_device: PciDevice,
    /// Active transport variant determined at probe time.
    pub kind: TransportKind,
}

impl VirtioPciTransport {
    // ─── Construction ───────────────────────────────────────────

    /// Probe a PCI device and initialize the VirtIO transport.
    ///
    /// Attempts modern transport first (VirtIO 1.0+, identified by VirtIO
    /// vendor-specific PCI capabilities). Falls back to the legacy transport
    /// when no modern capabilities are found and BAR0 is an I/O-space or
    /// memory-mapped region.
    ///
    /// Returns `Err` if neither transport can be established.
    pub fn probe(pci_device: PciDevice) -> Result<Self, ostd::Error> {
        pci_device.enable_bus_mastering();

        // Try modern transport first.
        let virtio_caps = parse_virtio_capabilities(&pci_device);
        if let Ok(transport) = Self::probe_modern(&pci_device, &virtio_caps) {
            return Ok(transport);
        }

        // Fall back to legacy transport.
        Self::probe_legacy(pci_device)
    }

    /// Attempt to set up a modern (VirtIO 1.0+) transport.
    ///
    /// Requires COMMON_CFG, NOTIFY_CFG, ISR_CFG, and DEVICE_CFG capabilities.
    fn probe_modern(
        pci_device: &PciDevice,
        caps: &[VirtioPciCapability],
    ) -> Result<Self, ostd::Error> {
        let common_cap =
            find_cap(caps, regs::CAP_TYPE_COMMON_CFG).ok_or(ostd::Error::NotEnoughResources)?;
        let notify_cap =
            find_cap(caps, regs::CAP_TYPE_NOTIFY_CFG).ok_or(ostd::Error::NotEnoughResources)?;
        let isr_cap =
            find_cap(caps, regs::CAP_TYPE_ISR_CFG).ok_or(ostd::Error::NotEnoughResources)?;
        let device_cfg_cap =
            find_cap(caps, regs::CAP_TYPE_DEVICE_CFG).ok_or(ostd::Error::NotEnoughResources)?;

        pci_device.enable_memory_space();

        let common_bar = map_bar(pci_device, common_cap)?;
        let notify_bar = map_bar(pci_device, notify_cap)?;
        let isr_bar = map_bar(pci_device, isr_cap)?;
        let device_cfg_bar = map_bar(pci_device, device_cfg_cap)?;

        let notify_offset_multiplier = notify_cap.extra;

        Ok(VirtioPciTransport {
            pci_device: pci_device.clone(),
            kind: TransportKind::Modern {
                common_bar,
                notify_bar,
                notify_offset_multiplier,
                isr_bar,
                device_cfg_bar,
            },
        })
    }

    /// Attempt to set up a legacy transport.
    ///
    /// Requires BAR0 to be either an I/O-space or memory-mapped region.
    fn probe_legacy(pci_device: PciDevice) -> Result<Self, ostd::Error> {
        let (io_base, bar_size) = match pci_device.bars[0] {
            PciBar::IoSpace { port, size } if port != 0 => (port as usize, size as usize),
            PciBar::MemoryMapped {
                base_addr, size, ..
            } if base_addr != 0 && size > 0 => (base_addr as usize, size as usize),
            _ => return Err(ostd::Error::InvalidArgs),
        };

        pci_device.enable_io_space();
        pci_device.enable_memory_space();

        // Map at least 256 bytes to cover the full legacy register layout.
        let map_size = core::cmp::max(bar_size, 0x100);
        let io_bar = IoMem::acquire(io_base..io_base + map_size)?;

        Ok(VirtioPciTransport {
            pci_device,
            kind: TransportKind::Legacy { io_bar },
        })
    }

    // ─── Device status ──────────────────────────────────────────

    /// Reset the device by writing 0 to the status register.
    pub fn reset(&self) -> Result<(), ostd::Error> {
        self.write_device_status(regs::STATUS_RESET)
    }

    /// Read the device status register.
    pub fn get_status(&self) -> Result<u8, ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => io_bar.read_once(regs::LEGACY_DEVICE_STATUS),
            TransportKind::Modern { common_bar, .. } => {
                common_bar.read_once(regs::COMMON_DEVICE_STATUS)
            }
        }
    }

    /// Write to the device status register.
    pub fn set_status(&self, status: u8) -> Result<(), ostd::Error> {
        self.write_device_status(status)
    }

    /// OR `bits` into the current device status (accumulating status bits).
    pub fn add_status(&self, bits: u8) -> Result<(), ostd::Error> {
        let current = self.get_status()?;
        self.write_device_status(current | bits)
    }

    // ─── Feature negotiation ────────────────────────────────────

    /// Read the device-offered feature bitmap as a 64-bit value.
    ///
    /// For modern devices, two 32-bit reads select words 0 and 1.
    /// For legacy devices, only 32 feature bits are available (word 0).
    pub fn read_device_features(&self) -> Result<u64, ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => {
                let lo: u32 = io_bar.read_once(regs::LEGACY_DEVICE_FEATURES)?;
                Ok(lo as u64)
            }
            TransportKind::Modern { common_bar, .. } => {
                // Select word 0 (bits 0–31)
                common_bar.write_once(regs::COMMON_DEVICE_FEATURE_SELECT, &0u32)?;
                let lo: u32 = common_bar.read_once(regs::COMMON_DEVICE_FEATURE)?;

                // Select word 1 (bits 32–63)
                common_bar.write_once(regs::COMMON_DEVICE_FEATURE_SELECT, &1u32)?;
                let hi: u32 = common_bar.read_once(regs::COMMON_DEVICE_FEATURE)?;

                Ok(((hi as u64) << 32) | (lo as u64))
            }
        }
    }

    /// Write the driver-accepted feature bitmap.
    ///
    /// Must be called after reading device features and before setting
    /// `STATUS_FEATURES_OK`. For modern devices, writes two 32-bit words.
    /// For legacy devices, the high 32 bits are ignored.
    pub fn write_driver_features(&self, features: u64) -> Result<(), ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => {
                let lo = features as u32;
                io_bar.write_once(regs::LEGACY_DRIVER_FEATURES, &lo)
            }
            TransportKind::Modern { common_bar, .. } => {
                let lo = features as u32;
                let hi = (features >> 32) as u32;

                // Write word 0
                common_bar.write_once(regs::COMMON_DRIVER_FEATURE_SELECT, &0u32)?;
                common_bar.write_once(regs::COMMON_DRIVER_FEATURE, &lo)?;

                // Write word 1
                common_bar.write_once(regs::COMMON_DRIVER_FEATURE_SELECT, &1u32)?;
                common_bar.write_once(regs::COMMON_DRIVER_FEATURE, &hi)
            }
        }
    }

    // ─── Queue configuration ────────────────────────────────────

    /// Select the virtqueue to configure.
    ///
    /// All subsequent `queue_*` operations apply to the selected queue.
    pub fn select_queue(&self, index: u16) -> Result<(), ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => {
                io_bar.write_once(regs::LEGACY_QUEUE_SELECT, &index)
            }
            TransportKind::Modern { common_bar, .. } => {
                common_bar.write_once(regs::COMMON_QUEUE_SELECT, &index)
            }
        }
    }

    /// Read the maximum queue size reported by the device for the selected queue.
    ///
    /// Returns 0 if the queue is not supported.
    pub fn read_queue_size(&self) -> Result<u16, ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => io_bar.read_once(regs::LEGACY_QUEUE_SIZE),
            TransportKind::Modern { common_bar, .. } => {
                common_bar.read_once(regs::COMMON_QUEUE_SIZE)
            }
        }
    }

    /// Set the negotiated queue size (modern transport only).
    ///
    /// On legacy transport this is read-only; the device controls queue size.
    pub fn write_queue_size(&self, size: u16) -> Result<(), ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { .. } => Ok(()), // no-op on legacy
            TransportKind::Modern { common_bar, .. } => {
                common_bar.write_once(regs::COMMON_QUEUE_SIZE, &size)
            }
        }
    }

    /// Write the physical page frame number (PFN) of the virtqueue (legacy only).
    ///
    /// Legacy devices locate the virtqueue via a 4096-byte-aligned PFN.
    /// On modern devices this is a no-op; use `write_queue_descriptor_addr` etc.
    pub fn write_legacy_queue_pfn(&self, pfn: u32) -> Result<(), ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => io_bar.write_once(regs::LEGACY_QUEUE_PFN, &pfn),
            TransportKind::Modern { .. } => Ok(()), // no-op on modern
        }
    }

    /// Write the physical address of the descriptor table (modern transport).
    ///
    /// Writes the address as two 32-bit words (low then high).
    pub fn write_queue_descriptor_addr(&self, addr: u64) -> Result<(), ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { .. } => Ok(()), // handled via PFN on legacy
            TransportKind::Modern { common_bar, .. } => {
                common_bar.write_once(regs::COMMON_QUEUE_DESC_LO, &(addr as u32))?;
                common_bar.write_once(regs::COMMON_QUEUE_DESC_HI, &((addr >> 32) as u32))
            }
        }
    }

    /// Write the physical address of the available ring (modern transport).
    pub fn write_queue_avail_addr(&self, addr: u64) -> Result<(), ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { .. } => Ok(()),
            TransportKind::Modern { common_bar, .. } => {
                common_bar.write_once(regs::COMMON_QUEUE_AVAIL_LO, &(addr as u32))?;
                common_bar.write_once(regs::COMMON_QUEUE_AVAIL_HI, &((addr >> 32) as u32))
            }
        }
    }

    /// Write the physical address of the used ring (modern transport).
    pub fn write_queue_used_addr(&self, addr: u64) -> Result<(), ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { .. } => Ok(()),
            TransportKind::Modern { common_bar, .. } => {
                common_bar.write_once(regs::COMMON_QUEUE_USED_LO, &(addr as u32))?;
                common_bar.write_once(regs::COMMON_QUEUE_USED_HI, &((addr >> 32) as u32))
            }
        }
    }

    /// Enable the selected virtqueue (modern transport only).
    ///
    /// Must be called after configuring the queue descriptor, available, and
    /// used ring addresses. On legacy transport, queues activate via the PFN write.
    pub fn enable_queue(&self) -> Result<(), ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { .. } => Ok(()),
            TransportKind::Modern { common_bar, .. } => {
                common_bar.write_once(regs::COMMON_QUEUE_ENABLE, &1u16)
            }
        }
    }

    // ─── Notification ───────────────────────────────────────────

    /// Notify the device that new buffers are available in the given queue.
    ///
    /// For legacy transport: writes `queue_index` to the queue-notify register.
    /// For modern transport: writes `queue_index` to the notification region at
    /// the offset computed from `queue_notify_off` × `notify_offset_multiplier`.
    pub fn notify_queue(&self, queue_index: u16) -> Result<(), ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => {
                io_bar.write_once(regs::LEGACY_QUEUE_NOTIFY, &queue_index)
            }
            TransportKind::Modern {
                common_bar,
                notify_bar,
                notify_offset_multiplier,
                ..
            } => {
                // Read queue_notify_off for the selected queue from common config.
                let notify_off: u16 = common_bar.read_once(regs::COMMON_QUEUE_NOTIFY_OFF)?;
                let byte_offset =
                    (notify_off as u32).wrapping_mul(*notify_offset_multiplier) as usize;
                notify_bar.write_once(byte_offset, &queue_index)
            }
        }
    }

    // ─── ISR ────────────────────────────────────────────────────

    /// Read and clear the ISR status register.
    ///
    /// Returns a bitmask of [`regs::ISR_VIRTQUEUE`] and [`regs::ISR_CONFIG_CHANGE`].
    /// Reading this register atomically clears it on the device side, acknowledging
    /// the interrupt.
    pub fn read_isr(&self) -> Result<u8, ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => io_bar.read_once(regs::LEGACY_ISR_STATUS),
            TransportKind::Modern { isr_bar, .. } => isr_bar.read_once(0usize),
        }
    }

    // ─── Device-specific configuration region ───────────────────

    /// Read a `u8` from the device-specific configuration region.
    pub fn read_device_config_u8(&self, offset: usize) -> Result<u8, ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => {
                io_bar.read_once(regs::LEGACY_DEVICE_CONFIG_START + offset)
            }
            TransportKind::Modern { device_cfg_bar, .. } => device_cfg_bar.read_once(offset),
        }
    }

    /// Read a `u16` from the device-specific configuration region.
    pub fn read_device_config_u16(&self, offset: usize) -> Result<u16, ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => {
                io_bar.read_once(regs::LEGACY_DEVICE_CONFIG_START + offset)
            }
            TransportKind::Modern { device_cfg_bar, .. } => device_cfg_bar.read_once(offset),
        }
    }

    /// Read a `u32` from the device-specific configuration region.
    pub fn read_device_config_u32(&self, offset: usize) -> Result<u32, ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => {
                io_bar.read_once(regs::LEGACY_DEVICE_CONFIG_START + offset)
            }
            TransportKind::Modern { device_cfg_bar, .. } => device_cfg_bar.read_once(offset),
        }
    }

    /// Read a `u64` from the device-specific configuration region.
    ///
    /// Implemented as two 32-bit reads to avoid assumptions about 64-bit atomicity.
    ///
    /// Per the VirtIO spec §4.1.4.3, if the device supports `CONFIG_GENERATION`,
    /// the caller should re-read until `config_generation` is stable. This method
    /// does not enforce that; callers that require it should loop externally.
    pub fn read_device_config_u64(&self, offset: usize) -> Result<u64, ostd::Error> {
        let lo = self.read_device_config_u32(offset)? as u64;
        let hi = self.read_device_config_u32(offset + 4)? as u64;
        Ok((hi << 32) | lo)
    }

    /// Return `true` if the device is using the modern (VirtIO 1.0+) transport.
    pub fn is_modern(&self) -> bool {
        matches!(self.kind, TransportKind::Modern { .. })
    }

    // ─── Queue metadata ─────────────────────────────────────────

    /// Read the number of virtqueues supported by the device.
    ///
    /// Modern transport reads `num_queues` from the common config region.
    /// Legacy transport does not expose this field; returns `u16::MAX` as a
    /// sentinel meaning "probe each queue individually with `select_queue`
    /// + `read_queue_size`".
    pub fn read_num_queues(&self) -> Result<u16, ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { .. } => Ok(u16::MAX),
            TransportKind::Modern { common_bar, .. } => {
                common_bar.read_once(regs::COMMON_NUM_QUEUES)
            }
        }
    }

    /// Read the `queue_notify_off` for the currently selected queue.
    ///
    /// This is the per-queue multiplier offset used to compute the
    /// byte offset within the notification BAR:
    /// `byte_offset = queue_notify_off × notify_offset_multiplier`.
    ///
    /// On legacy transport there is a single shared notify register,
    /// so this returns 0.
    pub fn read_queue_notify_off(&self) -> Result<u16, ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { .. } => Ok(0),
            TransportKind::Modern { common_bar, .. } => {
                common_bar.read_once(regs::COMMON_QUEUE_NOTIFY_OFF)
            }
        }
    }

    // ─── Configuration generation ───────────────────────────────

    /// Read the configuration generation counter.
    ///
    /// Per the VirtIO spec §4.1.4.3, the driver must re-read the device
    /// config if the generation counter changes between reads. This is
    /// only meaningful on modern transport; legacy always returns 0.
    pub fn read_config_generation(&self) -> Result<u8, ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { .. } => Ok(0),
            TransportKind::Modern { common_bar, .. } => {
                common_bar.read_once(regs::COMMON_CONFIG_GENERATION)
            }
        }
    }

    // ─── Device-specific configuration writes ───────────────────

    /// Write a `u8` to the device-specific configuration region.
    pub fn write_device_config_u8(&self, offset: usize, value: u8) -> Result<(), ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => {
                io_bar.write_once(regs::LEGACY_DEVICE_CONFIG_START + offset, &value)
            }
            TransportKind::Modern { device_cfg_bar, .. } => {
                device_cfg_bar.write_once(offset, &value)
            }
        }
    }

    /// Write a `u16` to the device-specific configuration region.
    pub fn write_device_config_u16(&self, offset: usize, value: u16) -> Result<(), ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => {
                io_bar.write_once(regs::LEGACY_DEVICE_CONFIG_START + offset, &value)
            }
            TransportKind::Modern { device_cfg_bar, .. } => {
                device_cfg_bar.write_once(offset, &value)
            }
        }
    }

    /// Write a `u32` to the device-specific configuration region.
    pub fn write_device_config_u32(&self, offset: usize, value: u32) -> Result<(), ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => {
                io_bar.write_once(regs::LEGACY_DEVICE_CONFIG_START + offset, &value)
            }
            TransportKind::Modern { device_cfg_bar, .. } => {
                device_cfg_bar.write_once(offset, &value)
            }
        }
    }

    // ─── Private helpers ────────────────────────────────────────

    /// Write to the device status register (dispatches to legacy or modern).
    fn write_device_status(&self, status: u8) -> Result<(), ostd::Error> {
        match &self.kind {
            TransportKind::Legacy { io_bar } => {
                io_bar.write_once(regs::LEGACY_DEVICE_STATUS, &status)
            }
            TransportKind::Modern { common_bar, .. } => {
                common_bar.write_once(regs::COMMON_DEVICE_STATUS, &status)
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────
// BAR mapping helper
// ──────────────────────────────────────────────────────────────

/// Map the BAR region described by a VirtIO PCI capability.
///
/// Resolves the BAR base address from the `PciDevice`'s parsed BAR table,
/// then maps the sub-region `[bar_base + cap.offset, bar_base + cap.offset + cap.length)`.
fn map_bar(pci_device: &PciDevice, cap: &VirtioPciCapability) -> Result<IoMem, ostd::Error> {
    let bar_base = match pci_device.bars[cap.bar as usize] {
        PciBar::MemoryMapped { base_addr, .. } if base_addr != 0 => base_addr as usize,
        PciBar::IoSpace { port, .. } if port != 0 => port as usize,
        _ => return Err(ostd::Error::InvalidArgs),
    };

    let start = bar_base + cap.offset as usize;
    let end = start + cap.length as usize;

    // Map at least one page to satisfy alignment/granularity requirements.
    let map_end = core::cmp::max(end, start + 0x10);
    IoMem::acquire(start..map_end)
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    /// STATUS_RESET must be 0 so that writing it triggers a full device reset.
    #[ktest]
    fn test_status_reset_is_zero() {
        assert_eq!(regs::STATUS_RESET, 0u8);
    }

    /// Confirm the standard accumulation order of status bits.
    #[ktest]
    fn test_status_bit_values() {
        assert_eq!(regs::STATUS_ACKNOWLEDGE, 0x01);
        assert_eq!(regs::STATUS_DRIVER, 0x02);
        assert_eq!(regs::STATUS_DRIVER_OK, 0x04);
        assert_eq!(regs::STATUS_FEATURES_OK, 0x08);
        assert_eq!(regs::STATUS_NEEDS_RESET, 0x40);
        assert_eq!(regs::STATUS_FAILED, 0x80);
    }

    /// Confirm VirtIO vendor ID value.
    #[ktest]
    fn test_virtio_vendor_id() {
        assert_eq!(regs::VIRTIO_VENDOR_ID, 0x1AF4);
    }

    /// Confirm cap type constants are distinct and non-zero.
    #[ktest]
    fn test_cap_type_constants_distinct() {
        let types = [
            regs::CAP_TYPE_COMMON_CFG,
            regs::CAP_TYPE_NOTIFY_CFG,
            regs::CAP_TYPE_ISR_CFG,
            regs::CAP_TYPE_DEVICE_CFG,
            regs::CAP_TYPE_PCI_CFG,
        ];
        for (i, &a) in types.iter().enumerate() {
            assert_ne!(a, 0, "cap type at index {} should be non-zero", i);
            for (j, &b) in types.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "cap types at {} and {} must be distinct", i, j);
                }
            }
        }
    }
}
