use alloc::sync::Arc;
use crate::device::DEVICE_MANAGER;
use crate::fs::vfs::types::{FileOps, InodeOps, VfsError};

/// Inode for block devices registered in devfs.
pub struct BlockDeviceInode {
    pub device_name: &'static str,
}

impl InodeOps for BlockDeviceInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(BlockDeviceFileOps {
            device_name: self.device_name,
        }))
    }

    fn stat(&self) -> Result<crate::fs::vfs::types::Stat, VfsError> {
        Ok(crate::fs::vfs::types::Stat {
            mode: 0o060660, // S_IFBLK | 0660
            nlink: 1,
            ..Default::default()
        })
    }
}

pub struct BlockDeviceFileOps {
    pub device_name: &'static str,
}

impl FileOps for BlockDeviceFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let dm = DEVICE_MANAGER.read();
        for dev_arc in dm.get_devices() {
            let mut dev_lock = dev_arc.lock();
            if dev_lock.name() == self.device_name {
                if let Some(block_dev) = dev_lock.as_block_device_mut() {
                    let block_size = block_dev.block_size();
                    let block_id = (offset / block_size) as u64;
                    let mut read_bytes = 0;
                    let mut temp_buf = alloc::vec![0u8; block_size];

                    while read_bytes < buf.len() {
                        let current_block = block_id + (read_bytes / block_size) as u64;
                        block_dev
                            .read_block(current_block, &mut temp_buf)
                            .map_err(|_| VfsError::NotSupported)?;

                        let remaining = buf.len() - read_bytes;
                        let chunk = core::cmp::min(remaining, block_size);
                        buf[read_bytes..read_bytes + chunk].copy_from_slice(&temp_buf[..chunk]);
                        read_bytes += chunk;
                    }
                    return Ok(read_bytes);
                }
            }
        }
        Err(VfsError::NotFound)
    }

    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let dm = DEVICE_MANAGER.read();
        for dev_arc in dm.get_devices() {
            let mut dev_lock = dev_arc.lock();
            if dev_lock.name() == self.device_name {
                if let Some(block_dev) = dev_lock.as_block_device_mut() {
                    let block_size = block_dev.block_size();
                    let block_id = (offset / block_size) as u64;
                    let mut written_bytes = 0;

                    while written_bytes < buf.len() {
                        let current_block = block_id + (written_bytes / block_size) as u64;
                        let remaining = buf.len() - written_bytes;
                        let chunk = core::cmp::min(remaining, block_size);

                        if chunk == block_size {
                            block_dev
                                .write_block(
                                    current_block,
                                    &buf[written_bytes..written_bytes + chunk],
                                )
                                .map_err(|_| VfsError::NotSupported)?;
                        } else {
                            let mut temp_buf = alloc::vec![0u8; block_size];
                            block_dev
                                .read_block(current_block, &mut temp_buf)
                                .map_err(|_| VfsError::NotSupported)?;
                            temp_buf[..chunk]
                                .copy_from_slice(&buf[written_bytes..written_bytes + chunk]);
                            block_dev
                                .write_block(current_block, &temp_buf)
                                .map_err(|_| VfsError::NotSupported)?;
                        }
                        written_bytes += chunk;
                    }
                    return Ok(written_bytes);
                }
            }
        }
        Err(VfsError::NotFound)
    }
}
