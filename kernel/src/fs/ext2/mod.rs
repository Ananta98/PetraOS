pub mod dir;
pub mod file;
pub mod inode;
pub mod superblock;

use self::inode::{Ext2InodeOps, Ext2Volume};
use crate::fs::errno::VfsError;
use crate::fs::vfs::MOUNT_TABLE;
use crate::fs::vfs::filesystem::{FileSystem, SuperBlock};
use crate::fs::vfs::inode::{Inode, InodeType};
use crate::drivers::{Device, BlockDevice, DeviceType, DriverError};
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

/// Mount the Ext2 filesystem under `/mnt` using a memory-backed mock device.
pub fn mount_ext2() {
    static MOCK_DISK_REGISTERED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if !MOCK_DISK_REGISTERED.swap(true, core::sync::atomic::Ordering::SeqCst) {
        let mock_disk = MockDisk {
            data: include_bytes!("ext2_disk.img"),
        };
        let device_ref = Arc::new(crate::sync::spinlock::Spinlock::new(
            alloc::boxed::Box::new(mock_disk) as alloc::boxed::Box<dyn Device>
        ));
        crate::drivers::DEVICE_MANAGER.lock().register(device_ref);
    }

    let ext2_fs = crate::fs::ext2::Ext2Fs::new("ext2_disk");
    {
        let mut mt = MOUNT_TABLE.lock();
        mt.mount("/mnt", &ext2_fs)
            .expect("Failed to mount ext2 at /mnt");
    }
}
