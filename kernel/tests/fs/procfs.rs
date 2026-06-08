extern crate alloc;

// Tests for procfs: read-only entries, write rejection.

#[path = "../../src/limine.rs"]
pub mod limine;

#[path = "."]
pub mod sync {
    #[path = "."]
    pub mod spinlock {
        #[path = "../../src/sync/spinlock.rs"]
        pub mod impl_spinlock;
        pub use impl_spinlock::Spinlock;
    }
}

#[path = "."]
pub mod fs {
    #[path = "../../src/fs/errno.rs"]
    pub mod errno;
    #[path = "../../src/fs/flags.rs"]
    pub mod flags;

    #[path = "."]
    pub mod vfs {
        #[path = "../../src/fs/vfs/inode.rs"]
        pub mod inode;
        #[path = "../../src/fs/vfs/dentry.rs"]
        pub mod dentry;
        #[path = "../../src/fs/vfs/file.rs"]
        pub mod file;
    }

    #[path = "."]
    pub mod procfs {
        #[path = "../../src/fs/procfs/inode.rs"]
        pub mod inode;
        #[path = "../../src/fs/procfs/entries.rs"]
        pub mod entries;
    }
}

use fs::errno::VfsError;
use fs::vfs::inode::{InodeOps, InodeType};
use fs::vfs::file::FileOps;
use fs::procfs::inode::{ProcDirInode, ProcFileInode};
use fs::procfs::entries::create_proc_entries;
use std::sync::Arc;
use core::sync::atomic::AtomicU64;

#[test]
fn test_proc_version_content() {
    let inode = ProcFileInode { content: b"PetraOS 0.1.0\n" };
    let ops = inode.open().expect("open should succeed");
    let mut buf = [0u8; 32];
    let read = ops.read(0, &mut buf).expect("read should succeed");
    let content = core::str::from_utf8(&buf[..read]).expect("valid utf-8");
    assert!(content.contains("PetraOS"));
}

#[test]
fn test_proc_file_write_rejected() {
    let inode = ProcFileInode { content: b"test" };
    let ops = inode.open().expect("open should succeed");
    let result = ops.write(0, b"data");
    assert_eq!(result.err(), Some(VfsError::ReadOnlyFs));
}

#[test]
fn test_proc_dir_create_rejected() {
    let dir = ProcDirInode::new();
    let result = dir.create("file.txt");
    assert_eq!(result.err(), Some(VfsError::ReadOnlyFs));
}

#[test]
fn test_proc_dir_mkdir_rejected() {
    let dir = ProcDirInode::new();
    let result = dir.mkdir("subdir");
    assert_eq!(result.err(), Some(VfsError::ReadOnlyFs));
}

#[test]
fn test_proc_entries_populated() {
    let dir = ProcDirInode::new();
    let next_ino = AtomicU64::new(10);
    create_proc_entries(&dir, &next_ino);

    let entries = dir.readdir().expect("readdir should succeed");
    assert!(entries.contains(&"version".into()));
    assert!(entries.contains(&"uptime".into()));
    assert!(entries.contains(&"meminfo".into()));
}

#[test]
fn test_proc_lookup_version() {
    let dir = ProcDirInode::new();
    let next_ino = AtomicU64::new(10);
    create_proc_entries(&dir, &next_ino);

    let version_inode = dir.lookup("version").expect("version should exist");
    assert_eq!(version_inode.inode_type, InodeType::File);
}
