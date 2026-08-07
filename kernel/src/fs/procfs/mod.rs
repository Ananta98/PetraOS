pub mod entries;
pub mod inode;

use self::entries::create_proc_entries;
use self::inode::ProcDirInode;
use crate::fs::errno::VfsError;
use crate::fs::vfs::filesystem::{FileSystem, SuperBlock};
use crate::fs::vfs::inode::{Inode, InodeType};
use crate::fs::vfs::mount::MOUNT_TABLE;
use alloc::sync::Arc;
use core::sync::atomic::AtomicU64;

/// Process information filesystem, mounted at `/proc`.
///
/// Provides read-only pseudo-files exposing kernel and system information.
/// Write operations are rejected with `VfsError::ReadOnlyFs`.
pub struct ProcFs;

impl FileSystem for ProcFs {
    fn name(&self) -> &'static str {
        "procfs"
    }

    fn mount(&self) -> Result<SuperBlock, VfsError> {
        let next_ino = AtomicU64::new(1);
        let ino = next_ino.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        let root_dir = ProcDirInode::new();

        // Populate static entries
        create_proc_entries(&root_dir, &next_ino);

        let root_inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::Directory,
            ops: Arc::new(root_dir),
        });

        Ok(SuperBlock {
            fs_name: "procfs",
            root_inode,
            next_ino,
            read_only: true,
        })
    }
}

/// Mount the process status filesystem at `/proc`.
pub fn mount_procfs() {
    let mut mt = MOUNT_TABLE.lock();
    mt.mount("/proc", &ProcFs)
        .expect("Failed to mount procfs at /proc");
}
