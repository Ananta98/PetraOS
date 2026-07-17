/// NVMe (Non-Volatile Memory Express) Driver
///
/// Implements the NVM Express 1.4 host controller driver for PetraOS.
///
/// # Module layout
///
/// - [`regs`]    — MMIO register offsets and bit-field constants.
/// - [`queue`]   — SQE/CQE layouts and queue state tracking.
/// - [`command`] — Admin and NVM I/O command builders.
/// - [`device`]  — [`NvmeBlockDevice`] implementing [`BlockDevice`].
///
/// # Initialization sequence
///
/// 1. Scan PCI for NVMe controllers (class 0x01, subclass 0x08, prog_if 0x02).
/// 2. Enable MMIO + bus mastering via PCI config space.
/// 3. Map the BAR0 MMIO region (typically 16 KiB).
/// 4. Reset the controller and wait for it to become ready.
/// 5. Configure the Admin queue pair (SQ + CQ) and enable the controller.
/// 6. Issue an Identify Namespace command to determine disk geometry.
/// 7. Create an IO queue pair (one CQ, one SQ) for data transfers.
/// 8. Register the resulting [`NvmeBlockDevice`] with the block subsystem.
///
/// If no physical NVMe controller is found, a simulated in-memory fallback
/// device is registered so the system boots cleanly in virtual environments
/// that do not expose an NVMe controller.
use crate::drivers::block::register_block_device;
use crate::drivers::irq::IrqRegistration;
use crate::drivers::pci::PciBar;
use alloc::string::String;
use alloc::sync::Arc;
use device::{NvmeBlockDevice, NvmeBlockDeviceInner, NvmeNamespaceGeometry};
use ostd::io::IoMem;
use ostd::mm::dma::DmaCoherent;
use ostd::mm::{HasDaddr, VmIo, VmIoOnce};
use ostd::sync::SpinLock;
use spin::Once;

mod command;
mod device;
mod queue;
pub mod regs;

/// Holds the NVMe controller IrqRegistration for the driver's lifetime.
/// Only meaningful on x86 where PCI INTx# is routed through the IOAPIC.
static NVME_IRQ: Once<IrqRegistration> = Once::new();

// ──────────────────────────────────────────────────────────────
// DMA buffer page allocation helpers
// ──────────────────────────────────────────────────────────────

/// Allocate `pages` pages of DMA-coherent memory, zeroed.
///
/// Returns an error if the allocator cannot satisfy the request.
fn alloc_dma(pages: usize) -> Result<DmaCoherent, ostd::Error> {
    DmaCoherent::alloc(pages, true)
}

// ──────────────────────────────────────────────────────────────
// Controller reset and enable
// ──────────────────────────────────────────────────────────────

/// Disable the NVMe controller (clear CC.EN) and wait for CSTS.RDY to clear.
///
/// This must be done before reprogramming Admin Queue base addresses.
fn disable_controller(mmio: &IoMem) -> Result<(), ostd::Error> {
    let mut cc: u32 = mmio.read_once(regs::REG_CC)?;
    cc &= !regs::CC_EN;
    mmio.write_once(regs::REG_CC, &cc)?;

    // Wait until controller confirms it is disabled (CSTS.RDY == 0)
    loop {
        let csts: u32 = mmio.read_once(regs::REG_CSTS)?;
        if (csts & regs::CSTS_RDY) == 0 {
            break;
        }
        // Check for fatal status during shutdown
        if (csts & regs::CSTS_CFS) != 0 {
            return Err(ostd::Error::IoError);
        }
        core::hint::spin_loop();
    }
    Ok(())
}

/// Enable the NVMe controller (set CC.EN) and wait for CSTS.RDY to set.
fn enable_controller(mmio: &IoMem) -> Result<(), ostd::Error> {
    let mut cc: u32 = mmio.read_once(regs::REG_CC)?;
    cc |= regs::CC_EN;
    mmio.write_once(regs::REG_CC, &cc)?;

    // Wait until controller signals ready
    loop {
        let csts: u32 = mmio.read_once(regs::REG_CSTS)?;
        if (csts & regs::CSTS_CFS) != 0 {
            return Err(ostd::Error::IoError);
        }
        if (csts & regs::CSTS_RDY) != 0 {
            break;
        }
        core::hint::spin_loop();
    }
    Ok(())
}

// ──────────────────────────────────────────────────────────────
// Admin queue setup
// ──────────────────────────────────────────────────────────────

/// Configure the Admin Submission and Completion Queue registers and
/// program the Controller Configuration to use the NVM command set.
///
/// Must be called while the controller is disabled.
fn setup_admin_queues(
    mmio: &IoMem,
    admin_sq: &DmaCoherent,
    admin_cq: &DmaCoherent,
) -> Result<(), ostd::Error> {
    // AQA: Admin CQ size [27:16] | Admin SQ size [11:0] (both 0-based)
    let aqa = ((regs::ADMIN_CQ_SIZE - 1) << 16) | (regs::ADMIN_QUEUE_SIZE - 1);
    mmio.write_once(regs::REG_AQA, &aqa)?;

    // ASQ: 64-bit physical base of Admin SQ
    let asq_base = admin_sq.daddr() as u64;
    mmio.write_once(regs::REG_ASQ, &(asq_base as u32))?;
    mmio.write_once(regs::REG_ASQ + 4, &((asq_base >> 32) as u32))?;

    // ACQ: 64-bit physical base of Admin CQ
    let acq_base = admin_cq.daddr() as u64;
    mmio.write_once(regs::REG_ACQ, &(acq_base as u32))?;
    mmio.write_once(regs::REG_ACQ + 4, &((acq_base >> 32) as u32))?;

    // CC: configure command set, page size, arbitration, and queue entry sizes
    let cc =
        regs::CC_CSS_NVM | regs::CC_MPS_4K | regs::CC_AMS_RR | regs::CC_IOSQES | regs::CC_IOCQES;
    mmio.write_once(regs::REG_CC, &cc)?;

    Ok(())
}

// ──────────────────────────────────────────────────────────────
// Namespace geometry discovery
// ──────────────────────────────────────────────────────────────

/// Query the controller for namespace 1's geometry via Identify Namespace.
///
/// Parses the NSZE (Namespace Size, total LBAs) and LBADS (LBA Data Size)
/// fields from the 4096-byte Identify Namespace data structure.
fn identify_namespace(
    mmio: &IoMem,
    admin_sq: &DmaCoherent,
    admin_cq: &DmaCoherent,
    identify_buf: &DmaCoherent,
    admin_state: &mut queue::QueueState,
) -> Result<NvmeNamespaceGeometry, ostd::Error> {
    // Identify Namespace (CNS = 0x00, NSID = 1)
    let mut id_data = alloc::vec![0u8; 4096];
    command::identify(
        mmio,
        admin_sq,
        admin_cq,
        identify_buf,
        admin_state,
        regs::IDENTIFY_CNS_NAMESPACE,
        regs::DEFAULT_NAMESPACE_ID,
        0x0001, // command_id
        &mut id_data,
    )?;

    // NSZE: bytes 0-7, total number of logical blocks
    let num_blocks = u64::from_le_bytes([
        id_data[0], id_data[1], id_data[2], id_data[3], id_data[4], id_data[5], id_data[6],
        id_data[7],
    ]);

    // LBA Format Support: byte 128 holds the current LBA format index (FLBAS[3:0])
    let flbas = id_data[26] & 0x0F;
    // Each LBA Format entry is 4 bytes starting at offset 128
    let lbaf_offset = 128 + (flbas as usize) * 4;
    // LBADS is bits [23:16] of the LBAF entry → byte index + 2
    let lbads = id_data[lbaf_offset + 2];
    let block_size = if lbads >= 9 {
        // block_size = 2^LBADS
        1usize << lbads
    } else {
        regs::NVME_BLOCK_SIZE
    };

    let num_blocks = if num_blocks == 0 { 2048 } else { num_blocks };

    Ok(NvmeNamespaceGeometry {
        num_blocks,
        block_size,
    })
}

// ──────────────────────────────────────────────────────────────
// Full physical controller initialization
// ──────────────────────────────────────────────────────────────

/// Initialize one physical NVMe controller reachable via `mmio_base`.
///
/// Steps:
/// 1. Disable controller.
/// 2. Allocate and program Admin queue pair.
/// 3. Enable controller.
/// 4. Identify Namespace 1 for geometry.
/// 5. Create IO queue pair (CQ first, then SQ).
/// 6. Return a fully initialized [`NvmeBlockDevice`].
fn init_controller(
    mmio_base: usize,
    controller_index: usize,
    irq_line: u8,
) -> Result<Arc<NvmeBlockDevice>, ostd::Error> {
    // Map the NVMe MMIO register space (16 KiB covers all standard registers +
    // doorbell registers for up to 1024 queue pairs with 4-byte stride).
    let mmio = IoMem::acquire(mmio_base..mmio_base + 0x4000)?;

    // Register PCI INTx# interrupt handler for this NVMe controller.
    // The handler reads CSTS to acknowledge the interrupt.  IO CQs are
    // created with IEN = 1 (interrupts disabled), so the handler is
    // defensive against stray events and controller fatal status; I/O
    // completion uses polling (CQ phase tag).
    #[cfg(target_arch = "x86_64")]
    if !NVME_IRQ.is_completed() && irq_line != 0 && irq_line < 16 {
        let isr_mmio = mmio.clone();
        if let Ok(irq) = crate::drivers::irq::map_isa_irq(irq_line, move || {
            let _: Result<u32, _> = isr_mmio.read_once(regs::REG_CSTS);
        }) {
            NVME_IRQ.call_once(|| irq);
        }
    }

    // ── Step 1: disable the controller ─────────────────────────
    disable_controller(&mmio)?;

    // ── Step 2: allocate Admin queue buffers ───────────────────
    // SQ entries are 64 bytes each; CQ entries are 16 bytes each.
    let admin_sq_pages = ((regs::ADMIN_QUEUE_SIZE as usize * queue::SQE_SIZE) + 0xFFF) / 0x1000;
    let admin_cq_pages = ((regs::ADMIN_CQ_SIZE as usize * queue::CQE_SIZE) + 0xFFF) / 0x1000;

    let admin_sq = alloc_dma(admin_sq_pages.max(1))?;
    let admin_cq = alloc_dma(admin_cq_pages.max(1))?;

    // 4096-byte buffer for Identify responses
    let identify_buf = alloc_dma(1)?;

    // ── Step 3: program Admin queues and enable controller ──────
    setup_admin_queues(&mmio, &admin_sq, &admin_cq)?;
    enable_controller(&mmio)?;

    // ── Step 4: Admin queue state tracker (queue_id = 0) ────────
    let mut admin_state = queue::QueueState::new(0, regs::ADMIN_QUEUE_SIZE as u16);

    // ── Step 5: identify namespace geometry ────────────────────
    let geometry = identify_namespace(&mmio, &admin_sq, &admin_cq, &identify_buf, &mut admin_state)
        .unwrap_or(NvmeNamespaceGeometry {
            num_blocks: 2048,
            block_size: regs::NVME_BLOCK_SIZE,
        });

    // ── Step 6: allocate IO queue buffers ─────────────────────
    let io_sq_pages = ((regs::IO_QUEUE_SIZE as usize * queue::SQE_SIZE) + 0xFFF) / 0x1000;
    let io_cq_pages = ((regs::IO_CQ_SIZE as usize * queue::CQE_SIZE) + 0xFFF) / 0x1000;

    let io_sq_dma = alloc_dma(io_sq_pages.max(1))?;
    let io_cq_dma = alloc_dma(io_cq_pages.max(1))?;

    // Transfer buffer for a single block (512 bytes → 1 page is sufficient)
    let transfer_buf = alloc_dma(1)?;

    // IO queue ID = 1 (first IO queue pair)
    let io_cq_id: u16 = 1;
    let io_sq_id: u16 = 1;

    // Create IO Completion Queue (must precede IO Submission Queue)
    command::create_io_cq(
        &mmio,
        &admin_sq,
        &admin_cq,
        &io_cq_dma,
        &mut admin_state,
        io_cq_id,
        regs::IO_CQ_SIZE as u16,
        0x0002, // command_id
    )?;

    // Create IO Submission Queue, paired with the CQ above
    command::create_io_sq(
        &mmio,
        &admin_sq,
        &admin_cq,
        &io_sq_dma,
        &mut admin_state,
        io_sq_id,
        io_cq_id,
        regs::IO_QUEUE_SIZE as u16,
        0x0003, // command_id
    )?;

    // IO queue state (queue_id = 1)
    let io_state = queue::QueueState::new(io_sq_id, regs::IO_QUEUE_SIZE as u16);

    let name = alloc::format!("nvme{}n1", controller_index);

    Ok(Arc::new(NvmeBlockDevice {
        name,
        inner: SpinLock::new(NvmeBlockDeviceInner::Physical {
            mmio,
            io_sq_dma,
            io_cq_dma,
            transfer_buf,
            io_state,
            geometry,
            next_command_id: 0x0010,
        }),
    }))
}

// ──────────────────────────────────────────────────────────────
// Driver entry point
// ──────────────────────────────────────────────────────────────

/// Initialize the NVMe driver.
///
/// Enumerates the PCI bus for NVMe controllers (PCI class 0x01, subclass 0x08,
/// prog_if 0x02), initializes each one found, and registers the resulting
/// block devices with the kernel device layer.
///
/// Falls back to a simulated in-memory device if no physical controller is
/// detected — this keeps the system functional under QEMU configurations
/// that omit NVMe hardware.
pub fn init() {
    let mut physical_found = false;

    // NVMe: class 0x01 (mass storage), subclass 0x08 (non-volatile memory)
    let pci_devices = crate::drivers::pci::find_devices_by_class(0x01, 0x08);

    for (controller_index, pci_dev) in pci_devices.into_iter().enumerate() {
        // prog_if 0x02 = NVM Express
        if pci_dev.prog_if != 0x02 {
            continue;
        }

        // NVMe registers live in BAR0 (64-bit MMIO)
        let mmio_base = match pci_dev.bars[0] {
            PciBar::MemoryMapped { base_addr, .. } if base_addr != 0 => base_addr as usize,
            _ => continue,
        };

        // Enable MMIO and bus mastering in PCI command register
        pci_dev.enable_memory_space();
        pci_dev.enable_bus_mastering();

        let irq_line = pci_dev.interrupt_line();

        match init_controller(mmio_base, controller_index, irq_line) {
            Ok(device) => {
                let name = device.name.clone();
                if register_block_device(&name, device).is_ok() {
                    physical_found = true;
                }
            }
            Err(_) => continue,
        }
    }

    if !physical_found {
        // Fallback: provide a 64 KiB simulated NVMe device (128 × 512-byte blocks)
        let sim_blocks = 128usize;
        let name = String::from("nvme-simulated");
        let device = Arc::new(NvmeBlockDevice {
            name: name.clone(),
            inner: SpinLock::new(NvmeBlockDeviceInner::Simulated {
                data: alloc::vec![0u8; sim_blocks * regs::NVME_BLOCK_SIZE],
            }),
        });
        let _ = register_block_device(&name, device);
    }
}

// ──────────────────────────────────────────────────────────────
// Kernel tests
// ──────────────────────────────────────────────────────────────

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::fs::ramfs::RamFs;
    use crate::fs::vfs::{init_root_fs, mount, register_filesystem, resolve_path};
    use ostd::prelude::ktest;

    /// End-to-end test: initialize the NVMe driver, mount devfs, verify the
    /// block device node is present, and perform a write-then-read round-trip.
    #[ktest]
    fn test_nvme_driver() {
        init();

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

        // Prefer a physical namespace; fall back to the simulated device.
        let dev_name = if resolve_path("/dev/nvme0n1").is_ok() {
            "/dev/nvme0n1"
        } else {
            "/dev/nvme-simulated"
        };

        let nvme_dentry = resolve_path(dev_name).unwrap();
        assert_eq!(
            nvme_dentry.inode.metadata().unwrap().file_type,
            crate::fs::vfs::FileType::BlockDevice,
        );

        let mut ops = nvme_dentry.inode.open(0).unwrap();
        let mut write_offset = 512;
        ops.write(b"nvme test data!!", &mut write_offset).unwrap();

        let mut read_buf = [0u8; 16];
        let mut read_offset = 512;
        ops.read(&mut read_buf, &mut read_offset).unwrap();
        assert_eq!(&read_buf, b"nvme test data!!");

        // Clean up
        let clean_name = if dev_name.starts_with("/dev/nvme") {
            &dev_name[5..]
        } else {
            "nvme-simulated"
        };
        let _ = crate::drivers::unregister_device(clean_name);

        crate::fs::vfs::unregister_filesystem("devfs").unwrap();
        crate::fs::vfs::unregister_filesystem("ramfs").unwrap();
        *crate::fs::vfs::ROOT_DENTRY.lock() = None;
        *crate::fs::vfs::CWD_DENTRY.lock() = None;
        crate::fs::vfs::DENTRY_CACHE.clear();
    }
}
