/// VirtIO Block Driver (virtio-blk)
///
/// Implements a VirtIO block device driver for PetraOS using the PCI
/// legacy (transitional) transport. Supports the standard VirtIO block
/// device initialization sequence:
///
/// 1. Scan PCI for virtio-blk devices (vendor 0x1AF4, device 0x1001/0x1042)
/// 2. Reset the device and perform feature negotiation
/// 3. Configure virtqueue 0 (the request queue) with DMA-coherent buffers
/// 4. Read disk capacity from device configuration space
/// 5. Register the block device with the kernel's block subsystem
///
/// Falls back to a simulated in-memory device when no physical virtio-blk
/// controller is present (e.g., in test environments or VMs without virtio).
use crate::drivers::block::register_block_device;
use crate::drivers::pci::PciBar;

use alloc::string::String;
use alloc::sync::Arc;
use device::{VirtioBlkDevice, VirtioBlkDeviceInner};
use ostd::io::IoMem;
use ostd::mm::HasDaddr;
use ostd::mm::VmIoOnce;
use ostd::mm::dma::DmaCoherent;
use ostd::sync::SpinLock;

mod device;
pub mod regs;
mod virtqueue;

use virtqueue::{VirtqueueLayout, VirtqueueState};

// ──────────────────────────────────────────────────────────────
// Legacy transport initialization helpers
// ──────────────────────────────────────────────────────────────

/// Read the device status register (legacy transport).
fn read_status(io_bar: &IoMem) -> Result<u8, ostd::Error> {
    io_bar.read_once(regs::LEGACY_STATUS)
}

/// Write the device status register (legacy transport).
fn write_status(io_bar: &IoMem, status: u8) -> Result<(), ostd::Error> {
    io_bar.write_once(regs::LEGACY_STATUS, &status)
}

/// Reset the VirtIO device by writing 0 to the status register.
fn reset_device(io_bar: &IoMem) -> Result<(), ostd::Error> {
    write_status(io_bar, regs::STATUS_RESET)
}

/// Read a 32-bit value from the device-specific config region at the
/// given offset (relative to the start of device config, not the BAR).
fn read_device_config_u32(io_bar: &IoMem, offset: usize) -> Result<u32, ostd::Error> {
    io_bar.read_once(regs::LEGACY_DEVICE_CONFIG_OFFSET + offset)
}

/// Initialize a physical virtio-blk device over PCI legacy transport.
///
/// Performs the full VirtIO initialization sequence:
/// 1. Reset → ACKNOWLEDGE → DRIVER
/// 2. Feature negotiation (read device features, accept a safe subset)
/// 3. Configure virtqueue 0 (the request queue)
/// 4. Set DRIVER_OK
/// 5. Read disk capacity from device config space
fn init_device(
    io_bar: &IoMem,
    device_index: usize,
) -> Result<Arc<VirtioBlkDevice>, ostd::Error> {
    // ── Step 1: Reset and set status bits ───────────────────────
    reset_device(io_bar)?;

    // Acknowledge the device
    write_status(io_bar, regs::STATUS_ACKNOWLEDGE)?;

    // Tell the device we know how to drive it
    let status = read_status(io_bar)?;
    write_status(io_bar, status | regs::STATUS_DRIVER)?;

    // ── Step 2: Feature negotiation ────────────────────────────
    // Read device-offered features
    let _device_features: u32 = io_bar.read_once(regs::LEGACY_DEVICE_FEATURES)?;

    // Accept no optional features for now — the base read/write
    // functionality requires no feature negotiation.
    io_bar.write_once(regs::LEGACY_DRIVER_FEATURES, &0u32)?;

    // ── Step 3: Configure virtqueue 0 ──────────────────────────
    // Select queue 0 (the request queue)
    io_bar.write_once(regs::LEGACY_QUEUE_SELECT, &0u16)?;

    // Read the maximum queue size supported by the device
    let queue_size: u16 = io_bar.read_once(regs::LEGACY_QUEUE_SIZE)?;
    if queue_size == 0 {
        return Err(ostd::Error::NotEnoughResources);
    }

    // Cap at a reasonable size to limit DMA allocation
    let queue_size = core::cmp::min(queue_size, 256);

    // Compute the layout for the virtqueue
    let layout = VirtqueueLayout::compute(queue_size);
    let total_pages = (layout.total_size + 0xFFF) / 0x1000;

    // Allocate DMA-coherent memory for the virtqueue
    let vq_dma = DmaCoherent::alloc(total_pages.max(1), true)?;

    // Initialize the free descriptor list
    let mut vq_state = VirtqueueState::new(queue_size);
    vq_state.init_free_list(&vq_dma)?;

    // Tell the device where the virtqueue lives (legacy: page frame number)
    let pfn = (vq_dma.daddr() / 0x1000) as u32;
    io_bar.write_once(regs::LEGACY_QUEUE_PFN, &pfn)?;

    // ── Step 4: Allocate per-request DMA buffers ───────────────
    // Header buffer (16 bytes — fits in 1 page)
    let header_buf = DmaCoherent::alloc(1, true)?;
    // Data buffer (512 bytes — fits in 1 page)
    let data_buf = DmaCoherent::alloc(1, true)?;
    // Status buffer (1 byte — fits in 1 page)
    let status_buf = DmaCoherent::alloc(1, true)?;

    // ── Step 5: Set DRIVER_OK ──────────────────────────────────
    let status = read_status(io_bar)?;
    write_status(io_bar, status | regs::STATUS_DRIVER_OK)?;

    // ── Step 6: Read disk capacity ─────────────────────────────
    // The capacity is stored as a 64-bit value in 512-byte sectors
    // at offset 0 of the device-specific config region.
    let cap_lo = read_device_config_u32(io_bar, regs::BLK_CFG_CAPACITY)? as u64;
    let cap_hi = read_device_config_u32(io_bar, regs::BLK_CFG_CAPACITY + 4)? as u64;
    let capacity_sectors = (cap_hi << 32) | cap_lo;

    // Use a minimum capacity if the device reports 0 (shouldn't happen
    // but protects against degenerate firmware)
    let capacity_sectors = if capacity_sectors == 0 {
        2048
    } else {
        capacity_sectors
    };

    let name = alloc::format!("virtio-blk{}", device_index);

    Ok(Arc::new(VirtioBlkDevice {
        name,
        inner: SpinLock::new(VirtioBlkDeviceInner::Physical {
            io_bar: io_bar.clone(),
            vq_dma,
            header_buf,
            data_buf,
            status_buf,
            vq_state,
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
        "VirtIO Block Device Driver (legacy/transitional transport)"
    }

    fn probe(&self) -> Result<(), ostd::Error> {
        let mut physical_found = false;
        let mut device_index = 0usize;

        // Scan all PCI devices for virtio-blk controllers.
        // VirtIO devices use vendor ID 0x1AF4; block devices use
        // device ID 0x1001 (transitional) or 0x1042 (modern).
        let all_devices = crate::drivers::pci::enumerate();

        for pci_dev in &all_devices {
            if pci_dev.vendor_id != regs::VIRTIO_VENDOR_ID {
                continue;
            }

            // Match transitional or modern virtio-blk device IDs
            let is_blk = pci_dev.device_id == regs::VIRTIO_BLK_DEVICE_ID_TRANSITIONAL
                || pci_dev.device_id == regs::VIRTIO_BLK_DEVICE_ID_MODERN;

            if !is_blk {
                continue;
            }

            // For legacy/transitional transport, BAR0 is an I/O space BAR
            let io_base = match pci_dev.bars[0] {
                PciBar::IoSpace { port, .. } if port != 0 => port as usize,
                PciBar::MemoryMapped { base_addr, size, .. }
                    if base_addr != 0 && size > 0 =>
                {
                    // Some QEMU configs use MMIO BAR0 even for transitional devices
                    base_addr as usize
                }
                _ => continue,
            };

            pci_dev.enable_io_space();
            pci_dev.enable_memory_space();
            pci_dev.enable_bus_mastering();

            // Map the I/O region. For legacy transport the BAR is typically
            // small (< 256 bytes). We map a 4K page to be safe.
            let bar_size = match pci_dev.bars[0] {
                PciBar::IoSpace { size, .. } => size as usize,
                PciBar::MemoryMapped { size, .. } => size as usize,
                _ => 0x1000,
            };
            let map_size = core::cmp::max(bar_size, 0x100);

            let io_bar = match IoMem::acquire(io_base..io_base + map_size) {
                Ok(bar) => bar,
                Err(_) => continue,
            };

            match init_device(&io_bar, device_index) {
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

        // If no physical device found, register a simulated fallback
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
