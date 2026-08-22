use alloc::sync::Arc;
use crate::sync::spinlock::Spinlock;
use alloc::boxed::Box;
use crate::device::{Device, DEVICE_MANAGER};
use crate::fs::vfs::types::{FileOps, InodeOps, VfsError};

/// Inode for block devices registered in devfs.
pub struct BlockDeviceInode {
    pub device_name: &'static str,
}

impl InodeOps for BlockDeviceInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        // Resolve the device once at open-time and cache it in the FileOps.
        // This avoids a linear scan of DEVICE_MANAGER on every read/write call.
        let device = DEVICE_MANAGER
            .read()
            .get_by_name(self.device_name)
            .ok_or(VfsError::NotFound)?;

        Ok(Arc::new(BlockDeviceFileOps { device }))
    }

    fn stat(&self) -> Result<crate::fs::vfs::types::Stat, VfsError> {
        Ok(crate::fs::vfs::types::Stat {
            mode: 0o060660, // S_IFBLK | 0660
            nlink: 1,
            ..Default::default()
        })
    }
}

/// Per-open file operations for a block device node.
///
/// Holds the device reference resolved at open-time (not per-I/O lookup).
pub struct BlockDeviceFileOps {
    device: Arc<Spinlock<Box<dyn Device>>>,
}

impl FileOps for BlockDeviceFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let mut dev_lock = self.device.lock();
        let block_dev = dev_lock.as_block_device_mut().ok_or(VfsError::NotSupported)?;
        let block_size = block_dev.block_size();

        let mut read_bytes = 0;
        let mut temp_buf = alloc::vec![0u8; block_size];

        while read_bytes < buf.len() {
            let current_offset = offset + read_bytes;
            let block_id = (current_offset / block_size) as u64;
            let block_offset = current_offset % block_size;

            block_dev
                .read_block(block_id, &mut temp_buf)
                .map_err(|e| VfsError::DriverError(e))?;

            let remaining = buf.len() - read_bytes;
            let chunk = core::cmp::min(remaining, block_size - block_offset);
            buf[read_bytes..read_bytes + chunk]
                .copy_from_slice(&temp_buf[block_offset..block_offset + chunk]);
            read_bytes += chunk;
        }
        Ok(read_bytes)
    }

    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let mut dev_lock = self.device.lock();
        let block_dev = dev_lock.as_block_device_mut().ok_or(VfsError::NotSupported)?;
        let block_size = block_dev.block_size();

        let mut written_bytes = 0;
        let mut sector_buf = alloc::vec![0u8; block_size];

        while written_bytes < buf.len() {
            let current_offset = offset + written_bytes;
            let block_id = (current_offset / block_size) as u64;
            let block_offset = current_offset % block_size;

            let remaining = buf.len() - written_bytes;
            let chunk = core::cmp::min(remaining, block_size - block_offset);

            // Read-modify-write for partial-block writes.
            if block_offset != 0 || chunk < block_size {
                block_dev
                    .read_block(block_id, &mut sector_buf)
                    .map_err(|e| VfsError::DriverError(e))?;
            }

            sector_buf[block_offset..block_offset + chunk]
                .copy_from_slice(&buf[written_bytes..written_bytes + chunk]);

            block_dev
                .write_block(block_id, &sector_buf)
                .map_err(|e| VfsError::DriverError(e))?;

            written_bytes += chunk;
        }
        Ok(written_bytes)
    }
}
