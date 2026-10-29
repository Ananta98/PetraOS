// SPDX-License-Identifier: GPL-2.0

//! Linux `struct block_device_operations` and `struct gendisk` abstractions.
//!
//! Provides a Linux-style block device interface that adapts into PetraOS [`BlockDevice`].

use alloc::{string::String, sync::Arc};

use crate::drivers::{
    block::BlockDevice, register_blkdev, unregister_blkdev, BlockDeviceStats,
};
use crate::uapi::errno::Result;

/// Linux `struct block_device_operations`: operations for block drivers.
pub trait BlockDeviceOperations: Send + Sync + 'static {
    /// Reads `count` sectors into `buf` starting at `sector`.
    fn read_sectors(&self, sector: u64, count: usize, buf: &mut [u8]) -> Result<()>;

    /// Writes `count` sectors from `buf` starting at `sector`.
    fn write_sectors(&self, sector: u64, count: usize, buf: &[u8]) -> Result<()>;

    /// Flushes any cached writes to physical backing media.
    fn flush(&self) -> Result<()> {
        Ok(())
    }
}

/// Linux `struct gendisk`: represents an individual disk or block storage device.
pub struct GenDisk {
    name: String,
    capacity_sectors: u64,
    sector_size: usize,
    fops: Arc<dyn BlockDeviceOperations>,
}

impl GenDisk {
    /// Allocates and initializes a disk representation.
    pub fn new(
        name: impl Into<String>,
        capacity_sectors: u64,
        sector_size: usize,
        fops: Arc<dyn BlockDeviceOperations>,
    ) -> Self {
        Self {
            name: name.into(),
            capacity_sectors,
            sector_size: if sector_size > 0 { sector_size } else { 512 },
            fops,
        }
    }

    /// Name of the disk (e.g. `sda`, `vda`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sector capacity of the disk.
    pub fn capacity(&self) -> u64 {
        self.capacity_sectors
    }

    /// Sector size in bytes.
    pub fn sector_size(&self) -> usize {
        self.sector_size
    }
}

/// Adapter converting `GenDisk` into PetraOS `BlockDevice`.
struct GenDiskAdapter {
    disk: GenDisk,
}

impl BlockDevice for GenDiskAdapter {
    fn name(&self) -> String {
        self.disk.name.clone()
    }

    fn block_size(&self) -> usize {
        self.disk.sector_size
    }

    fn block_count(&self) -> u64 {
        self.disk.capacity_sectors
    }

    fn read_blocks(&self, block_id: u64, buf: &mut [u8]) -> Result<usize> {
        let bsize = self.block_size();
        let count = buf.len() / bsize;
        self.disk.fops.read_sectors(block_id, count, buf)?;
        Ok(count * bsize)
    }

    fn write_blocks(&self, block_id: u64, buf: &[u8]) -> Result<usize> {
        let bsize = self.block_size();
        let count = buf.len() / bsize;
        self.disk.fops.write_sectors(block_id, count, buf)?;
        Ok(count * bsize)
    }

    fn flush(&self) -> Result<()> {
        self.disk.fops.flush()
    }

    fn stats(&self) -> BlockDeviceStats {
        BlockDeviceStats::default()
    }
}

/// Linux `add_disk`: registers a block device disk into the system.
pub fn add_disk(disk: GenDisk) -> Result<()> {
    let adapter = Arc::new(GenDiskAdapter { disk });
    register_blkdev(adapter)
}

/// Linux `del_gendisk`: unregisters a block device disk by name.
pub fn del_gendisk(name: &str) -> Result<()> {
    unregister_blkdev(name)
}
