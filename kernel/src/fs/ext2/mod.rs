pub mod bitmap;
pub mod dir;
pub mod file;
pub mod inode;
pub mod superblock;

use self::inode::{Ext2InodeOps, Ext2Volume};
use crate::device::{BlockDevice, Device, DeviceType, DriverError};
use crate::fs::vfs::types::{FileSystem, Inode, InodeType, SuperBlock, VfsError};
use crate::sync::spinlock::Spinlock;
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
            read_only: false,
        })
    }
}

/// A memory-backed mock block device containing an ext2 disk image buffer.
pub struct MockDisk {
    pub data: Spinlock<::alloc::vec::Vec<u8>>,
    pub name: &'static str,
}

impl MockDisk {
    pub fn new(initial_data: &[u8], name: &'static str) -> Self {
        Self {
            data: Spinlock::new(initial_data.to_vec()),
            name,
        }
    }
}

impl Device for MockDisk {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Block
    }

    fn name(&self) -> &'static str {
        self.name
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
        let data = self.data.lock();
        if offset + buf.len() > data.len() {
            return Err(DriverError::Unsupported);
        }
        buf.copy_from_slice(&data[offset..offset + buf.len()]);
        Ok(buf.len())
    }

    fn write_block(&mut self, block_id: u64, buf: &[u8]) -> Result<usize, DriverError> {
        let block_size = self.block_size();
        let offset = block_id as usize * block_size;
        let mut data = self.data.lock();
        let end = offset + buf.len();
        if end > data.len() {
            data.resize(end, 0);
        }
        data[offset..end].copy_from_slice(buf);
        Ok(buf.len())
    }

    fn block_size(&self) -> usize {
        1024
    }
}
