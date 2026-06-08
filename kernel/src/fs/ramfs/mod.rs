pub mod inode;
pub mod file_ops;

use alloc::sync::Arc;
use core::sync::atomic::AtomicU64;
use crate::fs::errno::VfsError;
use crate::fs::vfs::filesystem::{FileSystem, SuperBlock};
use crate::fs::vfs::inode::{Inode, InodeType};
use self::inode::RamDirInode;

/// In-memory filesystem. Used as the root filesystem and as the backing
/// implementation for tmpfs.
pub struct RamFs;

impl FileSystem for RamFs {
    fn name(&self) -> &'static str {
        "ramfs"
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
            fs_name: "ramfs",
            root_inode,
            next_ino,
            read_only: false,
        })
    }
}
