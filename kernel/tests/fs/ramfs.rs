extern crate alloc;

// Tests for ramfs: create file, write, read back, directory ops.

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
    pub mod ramfs {
        #[path = "../../src/fs/ramfs/file_ops.rs"]
        pub mod file_ops;
        #[path = "../../src/fs/ramfs/inode.rs"]
        pub mod inode;
    }
}

use fs::errno::VfsError;
use fs::vfs::inode::{InodeOps, InodeType};
use fs::vfs::file::FileOps;
use fs::ramfs::inode::{RamDirInode, RamFileInode};

// ── RamDirInode tests ────────────────────────────────────────────────────────

#[test]
fn test_mkdir_creates_directory() {
    let dir = RamDirInode::new();
    let child = dir.mkdir("subdir").expect("mkdir should succeed");
    assert_eq!(child.inode_type, InodeType::Directory);
}

#[test]
fn test_mkdir_duplicate_fails() {
    let dir = RamDirInode::new();
    dir.mkdir("subdir").expect("first mkdir should succeed");
    let result = dir.mkdir("subdir");
    assert_eq!(result.err(), Some(VfsError::AlreadyExists));
}

#[test]
fn test_create_file() {
    let dir = RamDirInode::new();
    let file = dir.create("test.txt").expect("create should succeed");
    assert_eq!(file.inode_type, InodeType::File);
}

#[test]
fn test_create_duplicate_fails() {
    let dir = RamDirInode::new();
    dir.create("test.txt").expect("first create should succeed");
    let result = dir.create("test.txt");
    assert_eq!(result.err(), Some(VfsError::AlreadyExists));
}

#[test]
fn test_lookup_existing() {
    let dir = RamDirInode::new();
    dir.create("file.txt").expect("create should succeed");
    let result = dir.lookup("file.txt");
    assert!(result.is_ok());
}

#[test]
fn test_lookup_nonexistent() {
    let dir = RamDirInode::new();
    let result = dir.lookup("missing.txt");
    assert_eq!(result.err(), Some(VfsError::NotFound));
}

#[test]
fn test_readdir_lists_entries() {
    let dir = RamDirInode::new();
    dir.create("a.txt").unwrap();
    dir.mkdir("b").unwrap();
    dir.create("c.txt").unwrap();
    let entries = dir.readdir().expect("readdir should succeed");
    assert_eq!(entries.len(), 3);
    assert!(entries.contains(&"a.txt".into()));
    assert!(entries.contains(&"b".into()));
    assert!(entries.contains(&"c.txt".into()));
}

// ── RamFileInode + RamFileOps tests ─────────────────────────────────────────

#[test]
fn test_file_write_then_read() {
    let file_inode = RamFileInode::new();
    let ops = file_inode.open().expect("open should succeed");

    let data = b"Hello, PetraOS!";
    let written = ops.write(0, data).expect("write should succeed");
    assert_eq!(written, data.len());

    let mut buf = [0u8; 32];
    let read = ops.read(0, &mut buf).expect("read should succeed");
    assert_eq!(read, data.len());
    assert_eq!(&buf[..read], data);
}

#[test]
fn test_file_read_at_offset() {
    let file_inode = RamFileInode::new();
    let ops = file_inode.open().expect("open should succeed");

    ops.write(0, b"ABCDEFGH").expect("write should succeed");

    let mut buf = [0u8; 4];
    let read = ops.read(4, &mut buf).expect("read at offset should succeed");
    assert_eq!(read, 4);
    assert_eq!(&buf, b"EFGH");
}

#[test]
fn test_file_read_past_end_returns_zero() {
    let file_inode = RamFileInode::new();
    let ops = file_inode.open().expect("open should succeed");

    ops.write(0, b"short").expect("write should succeed");

    let mut buf = [0u8; 8];
    let read = ops.read(100, &mut buf).expect("read past end should return 0");
    assert_eq!(read, 0);
}

#[test]
fn test_file_write_extends_content() {
    let file_inode = RamFileInode::new();
    let ops = file_inode.open().expect("open should succeed");

    // Write at offset 10 on empty file should extend
    let written = ops.write(10, b"hi").expect("write should succeed");
    assert_eq!(written, 2);

    let mut buf = [0u8; 12];
    let read = ops.read(0, &mut buf).expect("read should succeed");
    assert_eq!(read, 12);
    assert_eq!(&buf[10..12], b"hi");
    assert_eq!(&buf[0..10], &[0u8; 10]); // Zeroed gap
}
