use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::fs::vfs::inode::{Inode, InodeType};
use super::inode::{ProcDirInode, ProcFileInode};

/// Populate the procfs root directory with static entries.
///
/// Current entries:
/// - `version` — PetraOS version string
/// - `uptime`  — uptime stub (always "0")
/// - `meminfo` — memory info stub
pub fn create_proc_entries(root: &ProcDirInode, next_ino: &AtomicU64) {
    let version_ino = next_ino.fetch_add(1, Ordering::Relaxed);
    root.add_entry("version", Arc::new(Inode {
        ino: version_ino,
        inode_type: InodeType::File,
        ops: Arc::new(ProcFileInode {
            content: b"PetraOS 0.1.0\n",
        }),
    }));

    let uptime_ino = next_ino.fetch_add(1, Ordering::Relaxed);
    root.add_entry("uptime", Arc::new(Inode {
        ino: uptime_ino,
        inode_type: InodeType::File,
        ops: Arc::new(ProcFileInode {
            content: b"0\n",
        }),
    }));

    let meminfo_ino = next_ino.fetch_add(1, Ordering::Relaxed);
    root.add_entry("meminfo", Arc::new(Inode {
        ino: meminfo_ino,
        inode_type: InodeType::File,
        ops: Arc::new(ProcFileInode {
            content: b"total: 0\nfree: 0\n",
        }),
    }));
}
