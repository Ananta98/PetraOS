use crate::fs::errno::VfsError;
use crate::fs::ramfs::inode::RamDirInode;
use crate::fs::vfs::filesystem::{FileSystem, SuperBlock};
use crate::fs::vfs::inode::{Inode, InodeType};
use crate::fs::vfs::mount::MOUNT_TABLE;
use alloc::sync::Arc;
use core::sync::atomic::AtomicU64;

/// Temporary filesystem, mounted at `/tmp`.
///
/// Functionally identical to ramfs but with a distinct filesystem name
/// and separate superblock for namespace isolation.
pub struct TmpFs;

impl FileSystem for TmpFs {
    fn name(&self) -> &'static str {
        "tmpfs"
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
            fs_name: "tmpfs",
            root_inode,
            next_ino,
            read_only: false,
        })
    }
}

/// Mount the temporary filesystem at `/tmp`.
pub fn mount_tmpfs() {
    let mut mt = MOUNT_TABLE.lock();
    mt.mount("/tmp", &TmpFs)
        .expect("Failed to mount tmpfs at /tmp");
}
