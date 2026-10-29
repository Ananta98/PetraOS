// SPDX-License-Identifier: GPL-2.0

//! `virtio-blk` PCI device glue and [`BlockDevice`] implementation.
//!
//! [`VirtioBlkDriver`] registers with the VirtIO bus and constructs one
//! [`VirtioBlk`] instance per attached PCI transport (`vda`, `vdb`, ...).

use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use core::mem::size_of;
use core::sync::atomic::{AtomicU32, Ordering};
use ostd::mm::{
    HasPaddr, PAGE_SIZE, VmIo, VmReader, VmWriter, dma::DmaCoherent, io::util::HasVmReaderWriter,
};
use spin::Mutex;

use super::protocol::{
    BlkConfig, BlkReqHdr, FEATURE_BLK_SIZE, FEATURE_FLUSH, FEATURE_RO, FEATURE_SEG_MAX,
    FEATURE_SIZE_MAX, FEATURE_TOPOLOGY, PCI_DEVICE_ID_VIRTIO_BLK, REQ_TYPE_FLUSH, REQ_TYPE_IN,
    REQ_TYPE_OUT, REQUEST_QUEUE, STATUS_PENDING, VIRTIO_BLK_SECTOR_SIZE, check_status,
};
use crate::drivers::bus::virtio::{
    SubmitBuffer, VIRTIO_F_VERSION_1, VirtQueue, VirtioDevice, VirtioTransport,
};
use crate::drivers::{
    BlockDevice, BlockDeviceStats,
    block::{BlockDeviceCounters, register_block_device},
};
use crate::uapi::errno::{Errno, Result};

/// Offset within the command staging page where the 1-byte status sits.
const STATUS_OFFSET: usize = size_of::<BlkReqHdr>();

/// Maximum bytes transferred in a single `virtio-blk` request chunk (64 KiB = 16 pages).
const MAX_TRANSFER_BYTES: usize = 16 * PAGE_SIZE;

/// Monotonic disk index counter for naming (`vda`, `vdb`, ...).
static NEXT_DISK_INDEX: AtomicU32 = AtomicU32::new(0);

/// Coherent DMA staging buffers used to serialize block requests.
struct RequestStaging {
    /// Holds `BlkReqHdr` at offset `0` and the `u8` status byte at `STATUS_OFFSET`.
    cmd_dma: DmaCoherent,
    /// Pre-allocated contiguous DMA data buffer (`MAX_TRANSFER_BYTES`).
    data_dma: DmaCoherent,
}

impl RequestStaging {
    fn new() -> Result<Self> {
        let data_frames = MAX_TRANSFER_BYTES / PAGE_SIZE;
        Ok(Self {
            cmd_dma: DmaCoherent::alloc(1, false).map_err(|_| Errno::ENOMEM)?,
            data_dma: DmaCoherent::alloc(data_frames, false).map_err(|_| Errno::ENOMEM)?,
        })
    }
}

/// One active `virtio-blk` block device bound to a PCI transport.
pub struct VirtioBlk {
    _transport: Arc<VirtioTransport>,
    request_queue: Arc<VirtQueue>,
    staging: Mutex<RequestStaging>,
    config: BlkConfig,
    disk_index: u32,
    name: String,
    block_count: u64,
    counters: BlockDeviceCounters,
}

impl VirtioBlk {
    /// Initializes a `VirtioBlk` device over an activated [`VirtioTransport`].
    pub fn new(transport: &Arc<VirtioTransport>) -> Result<Arc<Self>> {
        let Some(request_queue) = transport.queue(REQUEST_QUEUE) else {
            ostd::error!(
                "virtio-blk: missing request queue (activated queues = {})",
                transport.queues().len()
            );
            crate::return_errno!(ENODEV, "virtio-blk does not expose a request queue");
        };

        let config = BlkConfig::read(transport)?;
        let sectors_per_block = (config.blk_size / VIRTIO_BLK_SECTOR_SIZE) as u64;
        let block_count = config.capacity_sectors / sectors_per_block.max(1);
        if block_count == 0 {
            crate::return_errno!(ENODEV, "virtio-blk device reports zero capacity");
        }

        let disk_index = NEXT_DISK_INDEX.fetch_add(1, Ordering::Relaxed);
        let name = format_disk_name(disk_index);

        ostd::info!(
            "virtio-blk: initialized \"{}\" (capacity: {} sectors / {} MB, blk_size: {} B, ro: {}, flush: {})",
            name,
            config.capacity_sectors,
            (config.capacity_sectors * VIRTIO_BLK_SECTOR_SIZE as u64) / (1024 * 1024),
            config.blk_size,
            config.read_only,
            config.flush_supported,
        );

        Ok(Arc::new(Self {
            _transport: Arc::clone(transport),
            request_queue,
            staging: Mutex::new(RequestStaging::new()?),
            config,
            disk_index,
            name,
            block_count,
            counters: BlockDeviceCounters::new(),
        }))
    }

    /// Maximum single-segment transfer size allowed by both our staging buffer
    /// and the device's `size_max` limit.
    fn max_chunk_bytes(&self) -> usize {
        let limit = match self.config.size_max {
            Some(sz) if (sz as usize) >= self.config.blk_size => {
                (sz as usize).min(MAX_TRANSFER_BYTES)
            }
            _ => MAX_TRANSFER_BYTES,
        };
        (limit / self.config.blk_size) * self.config.blk_size
    }

    /// Converts a logical `block_id` into a 512-byte protocol sector offset.
    fn block_to_sector(&self, block_id: u64) -> u64 {
        let sectors_per_block = (self.config.blk_size / VIRTIO_BLK_SECTOR_SIZE) as u64;
        block_id * sectors_per_block
    }

    /// Executes a single chunk read or write through the staging DMA buffers.
    fn submit_rw_chunk(
        &self,
        staging: &RequestStaging,
        req_type: u32,
        sector: u64,
        len: usize,
    ) -> Result<()> {
        let hdr = BlkReqHdr::new(req_type, sector);
        staging
            .cmd_dma
            .write_val(0, &hdr)
            .map_err(|_| Errno::EIO)?;
        staging
            .cmd_dma
            .write_val(STATUS_OFFSET, &STATUS_PENDING)
            .map_err(|_| Errno::EIO)?;

        let cmd_paddr = staging.cmd_dma.paddr() as u64;
        let data_paddr = staging.data_dma.paddr() as u64;
        let is_read = req_type == REQ_TYPE_IN;

        self.request_queue.submit_wait(&[
            SubmitBuffer {
                addr: cmd_paddr,
                len: size_of::<BlkReqHdr>() as u32,
                writable: false,
            },
            SubmitBuffer {
                addr: data_paddr,
                len: len as u32,
                writable: is_read,
            },
            SubmitBuffer {
                addr: cmd_paddr + STATUS_OFFSET as u64,
                len: 1,
                writable: true,
            },
        ])?;

        let status: u8 = staging
            .cmd_dma
            .read_val(STATUS_OFFSET)
            .map_err(|_| Errno::EIO)?;
        check_status(status)
    }
}

impl BlockDevice for VirtioBlk {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn block_size(&self) -> usize {
        self.config.blk_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn read_blocks(&self, block_id: u64, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if buf.len() % self.config.blk_size != 0 {
            crate::return_errno!(EINVAL, "buffer length must be a multiple of block size");
        }
        let num_blocks = (buf.len() / self.config.blk_size) as u64;
        let end_block = block_id.checked_add(num_blocks).ok_or(Errno::EINVAL)?;
        if end_block > self.block_count {
            crate::return_errno!(EINVAL, "read exceeds virtio-blk device capacity");
        }

        let chunk_max = self.max_chunk_bytes();
        let staging = self.staging.lock();
        let mut current_sector = self.block_to_sector(block_id);
        let mut offset = 0;

        while offset < buf.len() {
            let chunk_len = (buf.len() - offset).min(chunk_max);
            self.submit_rw_chunk(&staging, REQ_TYPE_IN, current_sector, chunk_len)?;

            let mut reader = staging.data_dma.reader();
            let mut writer = VmWriter::from(&mut buf[offset..offset + chunk_len]);
            reader.read(&mut writer);

            offset += chunk_len;
            current_sector += (chunk_len / VIRTIO_BLK_SECTOR_SIZE) as u64;
        }

        let sectors = (buf.len() / VIRTIO_BLK_SECTOR_SIZE) as u64;
        self.counters.record_read(sectors);

        Ok(buf.len())
    }

    fn write_blocks(&self, block_id: u64, buf: &[u8]) -> Result<usize> {
        if self.config.read_only {
            crate::return_errno!(EROFS, "virtio-blk device is read-only");
        }
        if buf.is_empty() {
            return Ok(0);
        }
        if buf.len() % self.config.blk_size != 0 {
            crate::return_errno!(EINVAL, "buffer length must be a multiple of block size");
        }
        let num_blocks = (buf.len() / self.config.blk_size) as u64;
        let end_block = block_id.checked_add(num_blocks).ok_or(Errno::EINVAL)?;
        if end_block > self.block_count {
            crate::return_errno!(EINVAL, "write exceeds virtio-blk device capacity");
        }

        let chunk_max = self.max_chunk_bytes();
        let staging = self.staging.lock();
        let mut current_sector = self.block_to_sector(block_id);
        let mut offset = 0;

        while offset < buf.len() {
            let chunk_len = (buf.len() - offset).min(chunk_max);

            let mut reader = VmReader::from(&buf[offset..offset + chunk_len]);
            let mut writer = staging.data_dma.writer();
            writer.write(&mut reader);

            self.submit_rw_chunk(&staging, REQ_TYPE_OUT, current_sector, chunk_len)?;

            offset += chunk_len;
            current_sector += (chunk_len / VIRTIO_BLK_SECTOR_SIZE) as u64;
        }

        let sectors = (buf.len() / VIRTIO_BLK_SECTOR_SIZE) as u64;
        self.counters.record_write(sectors);

        Ok(buf.len())
    }

    fn flush(&self) -> Result<()> {
        if !self.config.flush_supported {
            return Ok(());
        }

        let staging = self.staging.lock();
        let hdr = BlkReqHdr::new(REQ_TYPE_FLUSH, 0);
        staging
            .cmd_dma
            .write_val(0, &hdr)
            .map_err(|_| Errno::EIO)?;
        staging
            .cmd_dma
            .write_val(STATUS_OFFSET, &STATUS_PENDING)
            .map_err(|_| Errno::EIO)?;

        let cmd_paddr = staging.cmd_dma.paddr() as u64;
        self.request_queue.submit_wait(&[
            SubmitBuffer {
                addr: cmd_paddr,
                len: size_of::<BlkReqHdr>() as u32,
                writable: false,
            },
            SubmitBuffer {
                addr: cmd_paddr + STATUS_OFFSET as u64,
                len: 1,
                writable: true,
            },
        ])?;

        let status: u8 = staging
            .cmd_dma
            .read_val(STATUS_OFFSET)
            .map_err(|_| Errno::EIO)?;
        check_status(status)
    }

    fn device_number(&self) -> (u32, u32) {
        (254, self.disk_index * 16)
    }

    fn stats(&self) -> BlockDeviceStats {
        self.counters.to_stats()
    }
}

/// Formats a disk index into the Linux `vdX` naming scheme (`vda`, `vdb`, ..., `vdz`, `vdaa`, ...).
fn format_disk_name(index: u32) -> String {
    if index < 26 {
        format!("vd{}", (b'a' + index as u8) as char)
    } else {
        let first = (b'a' + ((index / 26) - 1) as u8) as char;
        let second = (b'a' + (index % 26) as u8) as char;
        format!("vd{}{}", first, second)
    }
}

/// VirtIO bus registration adapter for `virtio-blk`.
pub struct VirtioBlkDriver;

impl VirtioDevice for VirtioBlkDriver {
    fn device_id(&self) -> u16 {
        PCI_DEVICE_ID_VIRTIO_BLK
    }

    fn name(&self) -> &'static str {
        "virtio-blk"
    }

    fn queue_count(&self) -> usize {
        1
    }

    fn accept_features(&self, device_features: u64) -> u64 {
        device_features
            & (VIRTIO_F_VERSION_1
                | FEATURE_SIZE_MAX
                | FEATURE_SEG_MAX
                | FEATURE_RO
                | FEATURE_BLK_SIZE
                | FEATURE_FLUSH
                | FEATURE_TOPOLOGY)
    }

    fn on_init(&self, transport: &Arc<VirtioTransport>) -> Result<()> {
        let blk = VirtioBlk::new(transport)?;
        let name = blk.name();
        register_block_device(&name, blk)?;
        Ok(())
    }
}
