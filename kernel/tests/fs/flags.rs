extern crate alloc;

// Tests for open flag enforcement (O_RDONLY, O_WRONLY, O_RDWR).

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

// Minimal fs module tree
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
}

use fs::errno::VfsError;
use fs::flags::{O_RDONLY, O_WRONLY, O_RDWR, can_read, can_write};
use fs::vfs::file::{File, FileOps};
use fs::vfs::inode::{Inode, InodeOps, InodeType};
use fs::vfs::dentry::Dentry;
use std::sync::Arc;

/// A trivial in-memory file ops for testing.
struct TestFileOps;

impl FileOps for TestFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let data = b"hello";
        let len = core::cmp::min(buf.len(), data.len());
        buf[..len].copy_from_slice(&data[..len]);
        Ok(len)
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        Ok(buf.len())
    }
}

/// Dummy inode ops (unused but needed to construct Inode).
struct DummyInodeOps;
impl InodeOps for DummyInodeOps {}

fn make_test_file(flags: u32) -> File {
    let inode = Arc::new(Inode {
        ino: 1,
        inode_type: InodeType::File,
        ops: Arc::new(DummyInodeOps),
    });
    let dentry = Arc::new(Dentry::new("test".into(), inode));
    File::new(dentry, flags, Arc::new(TestFileOps))
}

// ── Flag helper tests ──────────────────────────────────────────────────────

#[test]
fn test_can_read_rdonly() {
    assert!(can_read(O_RDONLY));
    assert!(!can_write(O_RDONLY));
}

#[test]
fn test_can_write_wronly() {
    assert!(!can_read(O_WRONLY));
    assert!(can_write(O_WRONLY));
}

#[test]
fn test_can_readwrite_rdwr() {
    assert!(can_read(O_RDWR));
    assert!(can_write(O_RDWR));
}

// ── File read/write enforcement tests ──────────────────────────────────────

#[test]
fn test_rdonly_file_cannot_write() {
    let file = make_test_file(O_RDONLY);
    let result = file.write(b"data");
    assert_eq!(result, Err(VfsError::PermissionDenied));
}

#[test]
fn test_rdonly_file_can_read() {
    let file = make_test_file(O_RDONLY);
    let mut buf = [0u8; 8];
    let result = file.read(&mut buf);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 5); // "hello"
}

#[test]
fn test_wronly_file_cannot_read() {
    let file = make_test_file(O_WRONLY);
    let mut buf = [0u8; 8];
    let result = file.read(&mut buf);
    assert_eq!(result, Err(VfsError::PermissionDenied));
}

#[test]
fn test_wronly_file_can_write() {
    let file = make_test_file(O_WRONLY);
    let result = file.write(b"data");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 4);
}

#[test]
fn test_rdwr_file_can_read_and_write() {
    let file = make_test_file(O_RDWR);

    let mut buf = [0u8; 8];
    let read_result = file.read(&mut buf);
    assert!(read_result.is_ok());

    let write_result = file.write(b"data");
    assert!(write_result.is_ok());
}
