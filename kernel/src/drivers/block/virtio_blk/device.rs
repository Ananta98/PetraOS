/// VirtIO Block Device
///
/// Provides the `VirtioBlkDevice` type that implements the `BlockDevice` trait,
/// supporting both physical virtio-blk hardware (via `VirtioPciTransport`) and
/// a simulated in-memory fallback device for environments without virtio support.
use super::regs;
use crate::drivers::block::BlockDevice;
use crate::drivers::bus::virtio_pci::{
    SplitVirtqueue, VirtioPciTransport, VirtqDescFlags, VirtqDescriptor,
};
use alloc::string::String;
use alloc::vec::Vec;
use ostd::mm::dma::DmaCoherent;
use ostd::mm::{HasDaddr, VmIo};
use ostd::sync::SpinLock;

// ──────────────────────────────────────────────────────────────
// Inner state: physical vs. simulated
// ──────────────────────────────────────────────────────────────

/// Inner mutable state of a [`VirtioBlkDevice`].
pub enum VirtioBlkDeviceInner {
    /// Backed by a physical virtio-blk device over the unified PCI transport.
    Physical {
        /// VirtIO PCI transport — supports both legacy (pre-1.0) and modern
        /// (VirtIO 1.0+) devices, selected automatically at probe time.
        transport: VirtioPciTransport,
        /// Virtqueue 0 (the sole request queue for virtio-blk).
        virtqueue: SplitVirtqueue,
        /// DMA buffer for the request header (16 bytes).
        header_buf: DmaCoherent,
        /// DMA buffer for data transfer (one sector at a time).
        data_buf: DmaCoherent,
        /// DMA buffer for the 1-byte status response.
        status_buf: DmaCoherent,
        /// Number of 512-byte sectors reported by the device.
        capacity_sectors: u64,
    },
    /// In-memory simulated backend for testing environments without virtio.
    Simulated {
        /// Raw byte storage.
        data: Vec<u8>,
    },
}

// ──────────────────────────────────────────────────────────────
// VirtioBlkDevice
// ──────────────────────────────────────────────────────────────

/// A VirtIO block device wrapping either physical PCI hardware or simulated storage.
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
                transport,
                virtqueue,
                header_buf,
                data_buf,
                status_buf,
                ..
            } => {
                // Build the request header: type=IN, reserved=0, sector=block_id.
                let mut header = [0u8; regs::BLK_REQ_HEADER_SIZE];
                header[0..4].copy_from_slice(&regs::VIRTIO_BLK_T_IN.to_le_bytes());
                // reserved bytes [4..8] are already 0
                header[8..16].copy_from_slice(&(block_id as u64).to_le_bytes());
                header_buf.write_bytes(0, &header)?;

                // Clear status byte to a sentinel so we can detect completion.
                status_buf.write_bytes(0, &[0xFF_u8])?;

                // Submit a 3-descriptor chain:
                //   [0] header — device-readable
                //   [1] data   — device-writable (device fills the read data)
                //   [2] status — device-writable (device writes the status byte)
                let head = virtqueue.add_buffer(&[
                    VirtqDescriptor {
                        address: header_buf.daddr() as u64,
                        length: regs::BLK_REQ_HEADER_SIZE as u32,
                        flags: VirtqDescFlags::NONE,
                    },
                    VirtqDescriptor {
                        address: data_buf.daddr() as u64,
                        length: regs::VIRTIO_BLK_SECTOR_SIZE as u32,
                        flags: VirtqDescFlags::WRITE,
                    },
                    VirtqDescriptor {
                        address: status_buf.daddr() as u64,
                        length: 1,
                        flags: VirtqDescFlags::WRITE,
                    },
                ])?;

                virtqueue.notify(transport)?;

                // Spin until the device returns the used element.
                loop {
                    if let Some(_) = virtqueue.pop_used()? {
                        break;
                    }
                    core::hint::spin_loop();
                }

                // Check status byte written by the device.
                let mut status_byte = [0u8; 1];
                status_buf.read_bytes(0, &mut status_byte)?;
                if status_byte[0] != regs::VIRTIO_BLK_S_OK {
                    return Err(ostd::Error::IoError);
                }

                // Copy data from DMA buffer to the caller's slice.
                let _ = head; // head desc index not needed after pop
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
                transport,
                virtqueue,
                header_buf,
                data_buf,
                status_buf,
                ..
            } => {
                // Build the request header: type=OUT, reserved=0, sector=block_id.
                let mut header = [0u8; regs::BLK_REQ_HEADER_SIZE];
                header[0..4].copy_from_slice(&regs::VIRTIO_BLK_T_OUT.to_le_bytes());
                header[8..16].copy_from_slice(&(block_id as u64).to_le_bytes());
                header_buf.write_bytes(0, &header)?;

                // Copy data to DMA transfer buffer before submitting.
                data_buf.write_bytes(0, &buf[..regs::VIRTIO_BLK_SECTOR_SIZE])?;

                // Clear status byte.
                status_buf.write_bytes(0, &[0xFF_u8])?;

                // Submit a 3-descriptor chain:
                //   [0] header — device-readable
                //   [1] data   — device-readable (driver supplies the write data)
                //   [2] status — device-writable
                let head = virtqueue.add_buffer(&[
                    VirtqDescriptor {
                        address: header_buf.daddr() as u64,
                        length: regs::BLK_REQ_HEADER_SIZE as u32,
                        flags: VirtqDescFlags::NONE,
                    },
                    VirtqDescriptor {
                        address: data_buf.daddr() as u64,
                        length: regs::VIRTIO_BLK_SECTOR_SIZE as u32,
                        flags: VirtqDescFlags::NONE, // device-readable for writes
                    },
                    VirtqDescriptor {
                        address: status_buf.daddr() as u64,
                        length: 1,
                        flags: VirtqDescFlags::WRITE,
                    },
                ])?;

                virtqueue.notify(transport)?;

                // Spin until the device returns the used element.
                loop {
                    if let Some(_) = virtqueue.pop_used()? {
                        break;
                    }
                    core::hint::spin_loop();
                }

                // Check status byte.
                let mut status_byte = [0u8; 1];
                status_buf.read_bytes(0, &mut status_byte)?;
                if status_byte[0] != regs::VIRTIO_BLK_S_OK {
                    return Err(ostd::Error::IoError);
                }

                let _ = head;
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
