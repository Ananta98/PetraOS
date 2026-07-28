/// VirtIO Block Device
///
/// Provides the `VirtioBlkDevice` type that implements the `BlockDevice` trait,
/// supporting both physical virtio-blk hardware (via PCI legacy transport) and
/// a simulated in-memory fallback device for environments without virtio support.
use super::regs;
use super::virtqueue::VirtqueueState;
use crate::drivers::block::BlockDevice;
use alloc::string::String;
use alloc::vec::Vec;
use ostd::io::IoMem;
use ostd::mm::VmIo;
use ostd::mm::dma::DmaCoherent;
use ostd::sync::SpinLock;

// ──────────────────────────────────────────────────────────────
// Inner state: physical vs. simulated
// ──────────────────────────────────────────────────────────────

/// Inner mutable state of a [`VirtioBlkDevice`].
pub enum VirtioBlkDeviceInner {
    /// Backed by a physical virtio-blk device over PCI legacy transport.
    Physical {
        /// Legacy I/O BAR for register access and queue notification.
        io_bar: IoMem,
        /// DMA-coherent region holding the virtqueue structures
        /// (descriptor table, available ring, used ring).
        vq_dma: DmaCoherent,
        /// DMA buffer for the request header (16 bytes).
        header_buf: DmaCoherent,
        /// DMA buffer for data transfer (one sector at a time).
        data_buf: DmaCoherent,
        /// DMA buffer for the 1-byte status response.
        status_buf: DmaCoherent,
        /// Virtqueue state tracker.
        vq_state: VirtqueueState,
        /// Number of 512-byte sectors reported by the device.
        capacity_sectors: u64,
    },
    /// In-memory simulated backend for testing.
    Simulated {
        /// Raw byte storage.
        data: Vec<u8>,
    },
}

// ──────────────────────────────────────────────────────────────
// VirtioBlkDevice
// ──────────────────────────────────────────────────────────────

/// A VirtIO block device that wraps either physical hardware or simulated storage.
pub struct VirtioBlkDevice {
    pub(super) name: String,
    pub(super) inner: SpinLock<VirtioBlkDeviceInner>,
}

impl BlockDevice for VirtioBlkDevice {
    fn block_size(&self) -> usize {
        regs::VIRTIO_BLK_SECTOR_SIZE
    }

    fn num_blocks(&self) -> usize {
        match &*self.inner.lock() {
            VirtioBlkDeviceInner::Physical {
                capacity_sectors, ..
            } => *capacity_sectors as usize,
            VirtioBlkDeviceInner::Simulated { data } => data.len() / regs::VIRTIO_BLK_SECTOR_SIZE,
        }
    }

    fn read_blocks(&self, block_id: usize, buf: &mut [u8]) -> Result<(), ostd::Error> {
        if buf.len() < regs::VIRTIO_BLK_SECTOR_SIZE {
            return Err(ostd::Error::InvalidArgs);
        }

        let mut inner = self.inner.lock();
        match &mut *inner {
            VirtioBlkDeviceInner::Physical {
                io_bar,
                vq_dma,
                header_buf,
                data_buf,
                status_buf,
                vq_state,
                ..
            } => {
                // Build the request header: type=IN, reserved=0, sector=block_id
                let mut header = [0u8; regs::BLK_REQ_HEADER_SIZE];
                header[0..4].copy_from_slice(&regs::VIRTIO_BLK_T_IN.to_le_bytes());
                // reserved is already 0
                header[8..16].copy_from_slice(&(block_id as u64).to_le_bytes());
                header_buf.write_bytes(0, &header)?;

                // Clear status byte
                status_buf.write_bytes(0, &[0xFF_u8])?;

                // Submit the request
                let head = vq_state.submit_request(
                    vq_dma,
                    header_buf,
                    data_buf,
                    status_buf,
                    regs::VIRTIO_BLK_SECTOR_SIZE,
                    false, // read
                    io_bar,
                )?;

                // Poll for completion
                let status = vq_state.poll_used(vq_dma, status_buf, head)?;
                if status != regs::VIRTIO_BLK_S_OK {
                    return Err(ostd::Error::IoError);
                }

                // Copy data from DMA buffer to caller's slice
                data_buf.read_bytes(0, &mut buf[..regs::VIRTIO_BLK_SECTOR_SIZE])
            }
            VirtioBlkDeviceInner::Simulated { data } => {
                let offset = block_id * regs::VIRTIO_BLK_SECTOR_SIZE;
                if offset + regs::VIRTIO_BLK_SECTOR_SIZE > data.len() {
                    return Err(ostd::Error::InvalidArgs);
                }
                buf[..regs::VIRTIO_BLK_SECTOR_SIZE]
                    .copy_from_slice(&data[offset..offset + regs::VIRTIO_BLK_SECTOR_SIZE]);
                Ok(())
            }
        }
    }

    fn write_blocks(&self, block_id: usize, buf: &[u8]) -> Result<(), ostd::Error> {
        if buf.len() < regs::VIRTIO_BLK_SECTOR_SIZE {
            return Err(ostd::Error::InvalidArgs);
        }

        let mut inner = self.inner.lock();
        match &mut *inner {
            VirtioBlkDeviceInner::Physical {
                io_bar,
                vq_dma,
                header_buf,
                data_buf,
                status_buf,
                vq_state,
                ..
            } => {
                // Build the request header: type=OUT, reserved=0, sector=block_id
                let mut header = [0u8; regs::BLK_REQ_HEADER_SIZE];
                header[0..4].copy_from_slice(&regs::VIRTIO_BLK_T_OUT.to_le_bytes());
                header[8..16].copy_from_slice(&(block_id as u64).to_le_bytes());
                header_buf.write_bytes(0, &header)?;

                // Copy data to DMA transfer buffer
                data_buf.write_bytes(0, &buf[..regs::VIRTIO_BLK_SECTOR_SIZE])?;

                // Clear status byte
                status_buf.write_bytes(0, &[0xFF_u8])?;

                // Submit the request
                let head = vq_state.submit_request(
                    vq_dma,
                    header_buf,
                    data_buf,
                    status_buf,
                    regs::VIRTIO_BLK_SECTOR_SIZE,
                    true, // write
                    io_bar,
                )?;

                // Poll for completion
                let status = vq_state.poll_used(vq_dma, status_buf, head)?;
                if status != regs::VIRTIO_BLK_S_OK {
                    return Err(ostd::Error::IoError);
                }

                Ok(())
            }
            VirtioBlkDeviceInner::Simulated { data } => {
                let offset = block_id * regs::VIRTIO_BLK_SECTOR_SIZE;
                if offset + regs::VIRTIO_BLK_SECTOR_SIZE > data.len() {
                    return Err(ostd::Error::InvalidArgs);
                }
                data[offset..offset + regs::VIRTIO_BLK_SECTOR_SIZE]
                    .copy_from_slice(&buf[..regs::VIRTIO_BLK_SECTOR_SIZE]);
                Ok(())
            }
        }
    }
}
