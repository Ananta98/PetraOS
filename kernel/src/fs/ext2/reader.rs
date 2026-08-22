//! Block Device Reader for Ext2 Filesystem
//!
//! Provides arbitrary byte-level reading and writing on top of an underlying
//! block device registered in the kernel's `DeviceManager`.

use crate::device::DEVICE_MANAGER;
use crate::fs::vfs::types::VfsError;

/// Helper to read/write arbitrary byte offsets from/to a named block device.
#[derive(Clone, Debug)]
pub struct BlockDeviceReader {
    pub device_name: &'static str,
}

impl BlockDeviceReader {
    pub fn new(device_name: &'static str) -> Self {
        Self { device_name }
    }

    /// Read up to `buf.len()` bytes starting at `offset` (in bytes).
    pub fn read_bytes(&self, offset: u64, buf: &mut [u8]) -> Result<(), VfsError> {
        let dev_arc = DEVICE_MANAGER
            .read()
            .get_by_name(self.device_name)
            .ok_or(VfsError::NotFound)?;

        let mut dev_lock = dev_arc.lock();
        let block_dev = dev_lock.as_block_device_mut().ok_or(VfsError::NotSupported)?;
        let sector_size = block_dev.block_size() as u64;

        let mut read_offset = offset;
        let mut buf_offset = 0;
        let mut sector_buf = alloc::vec![0u8; sector_size as usize];

        while buf_offset < buf.len() {
            let sector_id = read_offset / sector_size;
            let sector_offset = (read_offset % sector_size) as usize;

            block_dev
                .read_block(sector_id, &mut sector_buf)
                .map_err(VfsError::DriverError)?;

            let chunk_size = core::cmp::min(
                buf.len() - buf_offset,
                (sector_size - sector_offset as u64) as usize,
            );
            buf[buf_offset..buf_offset + chunk_size]
                .copy_from_slice(&sector_buf[sector_offset..sector_offset + chunk_size]);

            buf_offset += chunk_size;
            read_offset += chunk_size as u64;
        }

        Ok(())
    }

    /// Write `buf.len()` bytes starting at `offset` (in bytes).
    pub fn write_bytes(&self, offset: u64, buf: &[u8]) -> Result<(), VfsError> {
        let dev_arc = DEVICE_MANAGER
            .read()
            .get_by_name(self.device_name)
            .ok_or(VfsError::NotFound)?;

        let mut dev_lock = dev_arc.lock();
        let block_dev = dev_lock.as_block_device_mut().ok_or(VfsError::NotSupported)?;
        let sector_size = block_dev.block_size() as u64;

        let mut write_offset = offset;
        let mut buf_offset = 0;
        let mut sector_buf = alloc::vec![0u8; sector_size as usize];

        while buf_offset < buf.len() {
            let sector_id = write_offset / sector_size;
            let sector_offset = (write_offset % sector_size) as usize;
            let chunk_size = core::cmp::min(
                buf.len() - buf_offset,
                (sector_size - sector_offset as u64) as usize,
            );

            // Read-modify-write for partial block writes
            if sector_offset != 0 || chunk_size < sector_size as usize {
                block_dev
                    .read_block(sector_id, &mut sector_buf)
                    .map_err(VfsError::DriverError)?;
            }

            sector_buf[sector_offset..sector_offset + chunk_size]
                .copy_from_slice(&buf[buf_offset..buf_offset + chunk_size]);

            block_dev
                .write_block(sector_id, &sector_buf)
                .map_err(VfsError::DriverError)?;

            buf_offset += chunk_size;
            write_offset += chunk_size as u64;
        }

        Ok(())
    }
}
