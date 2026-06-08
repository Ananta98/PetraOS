extern crate alloc;

// Tests for the FdTable module.

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
    pub mod fd {
        #[path = "../../src/fs/fd/fd_table.rs"]
        pub mod fd_table;
        pub use fd_table::FdTable;
    }
}

use fs::errno::VfsError;
use fs::flags::O_RDWR;
use fs::vfs::file::{File, FileOps};
use fs::vfs::inode::{Inode, InodeOps, InodeType};
use fs::vfs::dentry::Dentry;
use fs::fd::FdTable;
use std::sync::Arc;

struct DummyFileOps;
impl FileOps for DummyFileOps {}

struct DummyInodeOps;
impl InodeOps for DummyInodeOps {}

fn make_dummy_file() -> Arc<File> {
    let inode = Arc::new(Inode {
        ino: 1,
        inode_type: InodeType::File,
        ops: Arc::new(DummyInodeOps),
    });
    let dentry = Arc::new(Dentry::new("test".into(), inode));
    Arc::new(File::new(dentry, O_RDWR, Arc::new(DummyFileOps)))
}

#[test]
fn test_fd_alloc_starts_at_3() {
    let table = FdTable::new();
    let fd1 = table.alloc(make_dummy_file());
    let fd2 = table.alloc(make_dummy_file());
    assert_eq!(fd1, 3);
    assert_eq!(fd2, 4);
}

#[test]
fn test_fd_get_returns_correct_file() {
    let table = FdTable::new();
    let file = make_dummy_file();
    let fd = table.alloc(file.clone());
    let retrieved = table.get(fd);
    assert!(retrieved.is_ok());
}

#[test]
fn test_fd_get_nonexistent_returns_bad_fd() {
    let table = FdTable::new();
    let result = table.get(99);
    assert_eq!(result.err(), Some(VfsError::BadFd));
}

#[test]
fn test_fd_close_succeeds() {
    let table = FdTable::new();
    let fd = table.alloc(make_dummy_file());
    let result = table.close(fd);
    assert!(result.is_ok());
}

#[test]
fn test_fd_close_then_get_returns_bad_fd() {
    let table = FdTable::new();
    let fd = table.alloc(make_dummy_file());
    table.close(fd).expect("close should succeed");
    let result = table.get(fd);
    assert_eq!(result.err(), Some(VfsError::BadFd));
}

#[test]
fn test_fd_double_close_returns_bad_fd() {
    let table = FdTable::new();
    let fd = table.alloc(make_dummy_file());
    table.close(fd).expect("first close should succeed");
    let result = table.close(fd);
    assert_eq!(result.err(), Some(VfsError::BadFd));
}

#[test]
fn test_setup_std_fds() {
    let table = FdTable::new();
    let file = make_dummy_file();
    table.setup_std_fds(file);

    // FDs 0, 1, 2 should be accessible
    assert!(table.get(0).is_ok());
    assert!(table.get(1).is_ok());
    assert!(table.get(2).is_ok());
}
