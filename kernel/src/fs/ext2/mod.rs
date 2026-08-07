pub mod dir;
pub mod file;
pub mod inode;
pub mod superblock;

use self::inode::{Ext2InodeOps, Ext2Volume};
use crate::device::{BlockDevice, Device, DeviceType, DriverError};
use crate::fs::vfs::mount::MOUNT_TABLE;
use crate::fs::vfs::types::{FileSystem, Inode, InodeType, SuperBlock, VfsError};
use alloc::sync::Arc;

/// Ext2 Filesystem driver wrapper mapping to the VFS.
pub struct Ext2Fs {
    pub device_name: &'static str,
}

impl Ext2Fs {
    pub fn new(device_name: &'static str) -> Self {
        Self { device_name }
    }
}

impl FileSystem for Ext2Fs {
    fn name(&self) -> &'static str {
        "ext2"
    }

    /// Mount the Ext2 partition matching the configured block device name.
    fn mount(&self) -> Result<SuperBlock, VfsError> {
        let volume = Arc::new(Ext2Volume::new(self.device_name)?);

        // Root inode for Ext2 is always 2
        let root_inode = Arc::new(Inode {
            ino: 2,
            inode_type: InodeType::Directory,
            ops: Arc::new(Ext2InodeOps {
                volume: volume.clone(),
                ino: 2,
            }),
        });

        Ok(SuperBlock {
            fs_name: "ext2",
            root_inode,
            next_ino: core::sync::atomic::AtomicU64::new(volume.sb.inodes_count as u64 + 1),
            read_only: true,
        })
    }
}

/// A memory-backed mock block device containing the ext2 image.
pub struct MockDisk {
    data: &'static [u8],
}

impl Device for MockDisk {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Block
    }

    fn name(&self) -> &'static str {
        "ext2_disk"
    }

    fn init(&mut self) -> Result<(), DriverError> {
        Ok(())
    }

    fn as_block_device(&self) -> Option<&dyn BlockDevice> {
        Some(self)
    }

    fn as_block_device_mut(&mut self) -> Option<&mut dyn BlockDevice> {
        Some(self)
    }
}

impl BlockDevice for MockDisk {
    fn read_block(&mut self, block_id: u64, buf: &mut [u8]) -> Result<usize, DriverError> {
        let block_size = self.block_size();
        let offset = block_id as usize * block_size;
        if offset + buf.len() > self.data.len() {
            return Err(DriverError::Unsupported);
        }
        buf.copy_from_slice(&self.data[offset..offset + buf.len()]);
        Ok(buf.len())
    }

    fn write_block(&mut self, _block_id: u64, _buf: &[u8]) -> Result<usize, DriverError> {
        Err(DriverError::Unsupported)
    }

    fn block_size(&self) -> usize {
        1024
    }
}
