/// VirtIO Block Driver (virtio-blk)
///
/// Implements a VirtIO block device driver for PetraOS using the shared
/// [`VirtioPciTransport`] and [`SplitVirtqueue`] from the `virtio_pci` bus layer.
/// Supports both the legacy (pre-1.0) and modern (VirtIO 1.0+) transport variants,
/// with modern support selected automatically via capability probing.
///
/// # Initialization sequence
///
/// 1. Scan PCI for virtio-blk devices (vendor 0x1AF4, device 0x1001 or 0x1042)
/// 2. Probe the `VirtioPciTransport` (tries modern first, falls back to legacy)
/// 3. Reset, ACKNOWLEDGE, DRIVER — standard VirtIO status sequence
/// 4. Feature negotiation (read device features, accept a safe subset)
/// 5. Configure virtqueue 0 via `SplitVirtqueue::new`
/// 6. Set FEATURES_OK, then DRIVER_OK
/// 7. Read disk capacity from device configuration space
/// 8. Register the block device with the kernel's block subsystem
///
/// Falls back to a simulated in-memory device when no physical virtio-blk
/// controller is present (e.g., in test environments or VMs without virtio).
use crate::drivers::block::register_block_device;
use crate::drivers::bus::virtio_pci::{SplitVirtqueue, VirtioPciTransport, regs as virtio_regs};

use alloc::string::String;
use alloc::sync::Arc;
use device::{VirtioBlkDevice, VirtioBlkDeviceInner};
use ostd::mm::HasDaddr;
use ostd::mm::dma::DmaCoherent;
use ostd::sync::SpinLock;

mod device;
pub mod regs;

// ──────────────────────────────────────────────────────────────
// Physical device initialization
// ──────────────────────────────────────────────────────────────

/// Initialize a physical virtio-blk device using the unified VirtIO PCI transport.
///
/// Performs the full VirtIO initialization sequence per spec §3.1:
/// 1. Reset → ACKNOWLEDGE → DRIVER
/// 2. Feature negotiation
/// 3. Configure virtqueue 0
/// 4. FEATURES_OK → DRIVER_OK
/// 5. Read disk capacity from device config
fn init_device(
    transport: VirtioPciTransport,
    device_index: usize,
) -> Result<Arc<VirtioBlkDevice>, ostd::Error> {
    // ── Step 1: Reset and set initial status bits ──────────────
    transport.reset()?;
    transport.add_status(virtio_regs::STATUS_ACKNOWLEDGE)?;
    transport.add_status(virtio_regs::STATUS_DRIVER)?;

    // ── Step 2: Feature negotiation ────────────────────────────
    // Read device-offered features and accept only VERSION_1 on modern,
    // or no optional features on legacy (base read/write requires none).
    let device_features = transport.read_device_features()?;
    let accepted_features = device_features & virtio_regs::VIRTIO_F_VERSION_1;
    transport.write_driver_features(accepted_features)?;

    // ── Step 3: Configure virtqueue 0 ──────────────────────────
    transport.select_queue(0)?;
    let raw_queue_size = transport.read_queue_size()?;
    if raw_queue_size == 0 {
        return Err(ostd::Error::NotEnoughResources);
    }
    // Cap at a reasonable size to limit DMA allocation.
    let queue_size = core::cmp::min(raw_queue_size, 256);

    let virtqueue = SplitVirtqueue::new(&transport, 0, queue_size)?;

    // ── Step 4: Lock in features and signal driver readiness ───
    transport.add_status(virtio_regs::STATUS_FEATURES_OK)?;

    // Verify the device accepted our feature set (mandatory for modern).
    let status = transport.get_status()?;
    if (status & virtio_regs::STATUS_FEATURES_OK) == 0 {
        // Feature negotiation rejected — mark FAILED and bail.
        let _ = transport.add_status(virtio_regs::STATUS_FAILED);
        return Err(ostd::Error::NotEnoughResources);
    }

    // ── Step 5: Allocate per-request DMA buffers ───────────────
    // Header buffer: 16 bytes for the virtio_blk_req header.
    let header_buf = DmaCoherent::alloc(1, true)?;
    // Data buffer: 512 bytes (one sector).
    let data_buf = DmaCoherent::alloc(1, true)?;
    // Status buffer: 1 byte for the device's completion status.
    let status_buf = DmaCoherent::alloc(1, true)?;

    transport.add_status(virtio_regs::STATUS_DRIVER_OK)?;

    // ── Step 6: Read disk capacity ─────────────────────────────
    // The capacity is a 64-bit value in 512-byte sectors at offset 0 of the
    // device config region. Read as two 32-bit words for portability.
    let cap_lo = transport.read_device_config_u32(regs::BLK_CFG_CAPACITY)? as u64;
    let cap_hi = transport.read_device_config_u32(regs::BLK_CFG_CAPACITY + 4)? as u64;
    let capacity_sectors = (cap_hi << 32) | cap_lo;

    // Guard against degenerate firmware reporting 0 capacity.
    let capacity_sectors = if capacity_sectors == 0 {
        2048
    } else {
        capacity_sectors
    };

    let name = alloc::format!("virtio-blk{}", device_index);

    Ok(Arc::new(VirtioBlkDevice {
        name,
        inner: SpinLock::new(VirtioBlkDeviceInner::Physical {
            transport,
            virtqueue,
            header_buf,
            data_buf,
            status_buf,
            capacity_sectors,
        }),
    }))
}

// ──────────────────────────────────────────────────────────────
// Driver entry point
// ──────────────────────────────────────────────────────────────

/// VirtIO Block device driver.
pub struct VirtioBlkDriver;

impl crate::device::Driver for VirtioBlkDriver {
    fn name(&self) -> &str {
        "virtio-blk"
    }

    fn bus_name(&self) -> &str {
        "pci"
    }

    fn description(&self) -> &str {
        "VirtIO Block Device Driver (legacy and modern PCI transport)"
    }

    fn probe(&self) -> Result<(), ostd::Error> {
        let mut physical_found = false;
        let mut device_index = 0usize;

        // Scan all PCI devices for virtio-blk controllers.
        // VirtIO devices use vendor ID 0x1AF4; block devices use
        // device ID 0x1001 (transitional) or 0x1042 (modern).
        let all_devices = crate::drivers::pci::enumerate();

        for pci_dev in all_devices {
            if pci_dev.vendor_id != regs::VIRTIO_VENDOR_ID {
                continue;
            }

            let is_blk = pci_dev.device_id == regs::VIRTIO_BLK_DEVICE_ID_TRANSITIONAL
                || pci_dev.device_id == regs::VIRTIO_BLK_DEVICE_ID_MODERN;

            if !is_blk {
                continue;
            }

            // VirtioPciTransport::probe handles modern/legacy detection and BAR
            // mapping automatically; we do not need to inspect BARs directly.
            let transport = match VirtioPciTransport::probe(pci_dev) {
                Ok(t) => t,
                Err(_) => continue,
            };

            match init_device(transport, device_index) {
                Ok(device) => {
                    let name = device.name.clone();
                    if register_block_device(&name, device).is_ok() {
                        physical_found = true;
                        device_index += 1;
                    }
                }
                Err(_) => continue,
            }
        }

        // If no physical device found, register a simulated fallback.
        if !physical_found {
            let sim_sectors = 128usize;
            let name = String::from("virtio-blk-simulated");
            let device = Arc::new(VirtioBlkDevice {
                name: name.clone(),
                inner: SpinLock::new(VirtioBlkDeviceInner::Simulated {
                    data: alloc::vec![0u8; sim_sectors * regs::VIRTIO_BLK_SECTOR_SIZE],
                }),
            });
            let _ = register_block_device(&name, device);
        }

        Ok(())
    }
}

// Independent Linux C-style driver registration
crate::module_driver!(VIRTIO_BLK_INITCALL, virtio_blk_driver_init, "virtio-blk", VirtioBlkDriver);

// ──────────────────────────────────────────────────────────────
// Kernel tests
// ──────────────────────────────────────────────────────────────

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::device::driver::Driver;
    use crate::fs::ramfs::RamFs;
    use crate::fs::vfs::{init_root_fs, mount, register_filesystem, resolve_path};
    use ostd::prelude::ktest;

    /// End-to-end test: initialize the virtio-blk driver, mount devfs,
    /// verify the block device node is present, and perform a write-then-read
    /// round-trip.
    #[ktest]
    fn test_virtio_blk_driver() {
        let _ = VirtioBlkDriver.probe();

        let ramfs = Arc::new(RamFs);
        let _ = register_filesystem(ramfs);
        let _ = init_root_fs("ramfs", &[]);

        let devfs = Arc::new(crate::fs::devfs::DevFs);
        let _ = register_filesystem(devfs);

        let root = crate::fs::vfs::ROOT_DENTRY
            .lock()
            .as_ref()
            .cloned()
            .unwrap();
        root.inode.mkdir("dev", 0o755).unwrap();

        mount("devfs", "/dev", 0, &[]).unwrap();

        // Prefer a physical device; fall back to the simulated device.
        let dev_name = if resolve_path("/dev/virtio-blk0").is_ok() {
            "/dev/virtio-blk0"
        } else {
            "/dev/virtio-blk-simulated"
        };

        let virtio_dentry = resolve_path(dev_name).unwrap();
        assert_eq!(
            virtio_dentry.inode.metadata().unwrap().file_type,
            crate::fs::vfs::FileType::BlockDevice,
        );

        let mut ops = virtio_dentry.inode.open(0).unwrap();
        let mut write_offset = 512;
        ops.write(b"virtio-blk test!", &mut write_offset).unwrap();

        let mut read_buf = [0u8; 16];
        let mut read_offset = 512;
        ops.read(&mut read_buf, &mut read_offset).unwrap();
        assert_eq!(&read_buf, b"virtio-blk test!");

        // Clean up
        let clean_name = if dev_name.starts_with("/dev/virtio-blk") {
            &dev_name[5..]
        } else {
            "virtio-blk-simulated"
        };
        let _ = crate::drivers::unregister_device(clean_name);

        crate::fs::vfs::unregister_filesystem("devfs").unwrap();
        crate::fs::vfs::unregister_filesystem("ramfs").unwrap();
        *crate::fs::vfs::ROOT_DENTRY.lock() = None;
        *crate::fs::vfs::CWD_DENTRY.lock() = None;
        crate::fs::vfs::DENTRY_CACHE.clear();
    }
}
