pub mod console;

use alloc::sync::Arc;
use core::sync::atomic::AtomicU64;
use crate::fs::errno::VfsError;
use crate::fs::vfs::filesystem::{FileSystem, SuperBlock};
use crate::fs::vfs::inode::{Inode, InodeType};
use crate::fs::ramfs::inode::RamDirInode;

/// Device filesystem, mounted at `/dev`.
///
/// Uses a `RamDirInode` as its root directory and exposes device inodes
/// registered via [`DevFs::register_device`].
pub struct DevFs;

impl FileSystem for DevFs {
    fn name(&self) -> &'static str {
        "devfs"
    }

    fn mount(&self) -> Result<SuperBlock, VfsError> {
        let next_ino = AtomicU64::new(1);
        let ino = next_ino.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        let root_inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::Directory,
            ops: Arc::new(RamDirInode::new()),
        });

        Ok(SuperBlock {
            fs_name: "devfs",
            root_inode,
            next_ino,
            read_only: false,
        })
    }
}
