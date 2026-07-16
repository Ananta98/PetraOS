/// NVMe Block Device
///
/// Implements the [`BlockDevice`] trait for both physical NVMe hardware and
/// an in-memory simulated fallback used when no controller is present.
///
/// Physical I/O is dispatched through the IO queue pair that was set up
/// during initialization, while the simulated variant provides a simple
/// byte-vector backend for testing.
use super::{command, queue, regs};
use crate::drivers::block::BlockDevice;
use alloc::string::String;
use alloc::vec::Vec;
use ostd::io::IoMem;
use ostd::mm::dma::DmaCoherent;
use ostd::sync::SpinLock;

// ──────────────────────────────────────────────────────────────
// Device identity parsed from Identify Namespace
// ──────────────────────────────────────────────────────────────

/// Geometry parsed from the Identify Namespace response (CNS = 0x00).
#[derive(Debug, Clone, Copy)]
pub struct NvmeNamespaceGeometry {
    /// Total number of logical blocks on this namespace.
    pub num_blocks: u64,
    /// Logical block size in bytes (LBA data size, LBADS field).
    pub block_size: usize,
}

// ──────────────────────────────────────────────────────────────
// Inner state: physical vs. simulated
// ──────────────────────────────────────────────────────────────

/// Inner mutable state of an [`NvmeBlockDevice`].
///
/// We use an enum so that the same type can represent either a real NVMe
/// namespace or a simulated in-memory device without heap allocation overhead.
pub enum NvmeBlockDeviceInner {
    /// Backed by a physical NVMe controller.
    Physical {
        /// MMIO region for the NVMe controller registers.
        mmio: IoMem,
        /// DMA-coherent buffer for the IO Submission Queue.
        io_sq_dma: DmaCoherent,
        /// DMA-coherent buffer for the IO Completion Queue.
        io_cq_dma: DmaCoherent,
        /// DMA-coherent transfer buffer (one block at a time).
        transfer_buf: DmaCoherent,
        /// Mutable IO queue state (tail/head/phase tracking).
        io_state: queue::QueueState,
        /// Namespace geometry.
        geometry: NvmeNamespaceGeometry,
        /// Rolling command identifier for deduplication.
        next_command_id: u16,
    },
    /// In-memory simulated backend for testing.
    Simulated { data: Vec<u8> },
}

// ──────────────────────────────────────────────────────────────
// NvmeBlockDevice
// ──────────────────────────────────────────────────────────────

/// An NVMe block device that wraps either physical hardware or simulated storage.
pub struct NvmeBlockDevice {
    pub(super) name: String,
    pub(super) inner: SpinLock<NvmeBlockDeviceInner>,
}

impl BlockDevice for NvmeBlockDevice {
    fn block_size(&self) -> usize {
        regs::NVME_BLOCK_SIZE
    }

    fn num_blocks(&self) -> usize {
        match &*self.inner.lock() {
            NvmeBlockDeviceInner::Physical { geometry, .. } => geometry.num_blocks as usize,
            NvmeBlockDeviceInner::Simulated { data } => data.len() / regs::NVME_BLOCK_SIZE,
        }
    }

    fn read_blocks(&self, block_id: usize, buf: &mut [u8]) -> Result<(), ostd::Error> {
        if buf.len() < regs::NVME_BLOCK_SIZE {
            return Err(ostd::Error::InvalidArgs);
        }
        let mut inner = self.inner.lock();
        match &mut *inner {
            NvmeBlockDeviceInner::Physical {
                mmio,
                io_sq_dma,
                io_cq_dma,
                transfer_buf,
                io_state,
                next_command_id,
                ..
            } => {
                let cid = *next_command_id;
                *next_command_id = next_command_id.wrapping_add(1);

                command::submit_io(
                    mmio,
                    io_sq_dma,
                    io_cq_dma,
                    transfer_buf,
                    io_state,
                    regs::DEFAULT_NAMESPACE_ID,
                    false, // read
                    block_id as u64,
                    1,
                    cid,
                )?;

                // Copy from DMA buffer into the caller's slice
                use ostd::mm::VmIo;
                transfer_buf.read_bytes(0, &mut buf[..regs::NVME_BLOCK_SIZE])
            }
            NvmeBlockDeviceInner::Simulated { data } => {
                let offset = block_id * regs::NVME_BLOCK_SIZE;
                if offset + regs::NVME_BLOCK_SIZE > data.len() {
                    return Err(ostd::Error::InvalidArgs);
                }
                buf[..regs::NVME_BLOCK_SIZE]
                    .copy_from_slice(&data[offset..offset + regs::NVME_BLOCK_SIZE]);
                Ok(())
            }
        }
    }

    fn write_blocks(&self, block_id: usize, buf: &[u8]) -> Result<(), ostd::Error> {
        if buf.len() < regs::NVME_BLOCK_SIZE {
            return Err(ostd::Error::InvalidArgs);
        }
        let mut inner = self.inner.lock();
        match &mut *inner {
            NvmeBlockDeviceInner::Physical {
                mmio,
                io_sq_dma,
                io_cq_dma,
                transfer_buf,
                io_state,
                next_command_id,
                ..
            } => {
                let cid = *next_command_id;
                *next_command_id = next_command_id.wrapping_add(1);

                // Copy from the caller's slice into the DMA transfer buffer
                use ostd::mm::VmIo;
                transfer_buf.write_bytes(0, &buf[..regs::NVME_BLOCK_SIZE])?;

                command::submit_io(
                    mmio,
                    io_sq_dma,
                    io_cq_dma,
                    transfer_buf,
                    io_state,
                    regs::DEFAULT_NAMESPACE_ID,
                    true, // write
                    block_id as u64,
                    1,
                    cid,
                )
            }
            NvmeBlockDeviceInner::Simulated { data } => {
                let offset = block_id * regs::NVME_BLOCK_SIZE;
                if offset + regs::NVME_BLOCK_SIZE > data.len() {
                    return Err(ostd::Error::InvalidArgs);
                }
                data[offset..offset + regs::NVME_BLOCK_SIZE]
                    .copy_from_slice(&buf[..regs::NVME_BLOCK_SIZE]);
                Ok(())
            }
        }
    }
}
