extern crate alloc;

// Tests for tmpfs (reuses ramfs internals).

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
        #[path = "../../src/fs/vfs/filesystem.rs"]
        pub mod filesystem;
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

use fs::vfs::filesystem::FileSystem;
use fs::vfs::inode::{InodeOps, InodeType};
use fs::vfs::file::FileOps;

/// Inline TmpFs since we can't use #[path] on mod.rs that has submodules.
struct TmpFs;

impl FileSystem for TmpFs {
    fn name(&self) -> &'static str { "tmpfs" }

    fn mount(&self) -> Result<fs::vfs::filesystem::SuperBlock, fs::errno::VfsError> {
        use std::sync::Arc;
        use core::sync::atomic::AtomicU64;
        let next_ino = AtomicU64::new(1);
        let ino = next_ino.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let root_inode = Arc::new(fs::vfs::inode::Inode {
            ino,
            inode_type: InodeType::Directory,
            ops: Arc::new(fs::ramfs::inode::RamDirInode::new()),
        });
        Ok(fs::vfs::filesystem::SuperBlock {
            fs_name: "tmpfs",
            root_inode,
            next_ino,
            read_only: false,
        })
    }
}

#[test]
fn test_tmpfs_mount_produces_directory_root() {
    let fs = TmpFs;
    let sb = fs.mount().expect("mount should succeed");
    assert_eq!(sb.fs_name, "tmpfs");
    assert_eq!(sb.root_inode.inode_type, InodeType::Directory);
}

#[test]
fn test_tmpfs_create_and_read_file() {
    let fs = TmpFs;
    let sb = fs.mount().expect("mount should succeed");

    // Create a file in the root
    let file_inode = sb.root_inode.ops.create("temp.txt").expect("create should succeed");
    assert_eq!(file_inode.inode_type, InodeType::File);

    // Open, write, read
    let ops = file_inode.ops.open().expect("open should succeed");
    let data = b"temporary data";
    ops.write(0, data).expect("write should succeed");

    let mut buf = [0u8; 32];
    let read = ops.read(0, &mut buf).expect("read should succeed");
    assert_eq!(&buf[..read], data);
}

#[test]
fn test_tmpfs_separate_from_other_instance() {
    let fs = TmpFs;
    let sb1 = fs.mount().expect("first mount should succeed");
    let sb2 = fs.mount().expect("second mount should succeed");

    // Create file in sb1
    sb1.root_inode.ops.create("file1.txt").expect("create in sb1 should succeed");

    // sb2 should not have file1.txt
    let result = sb2.root_inode.ops.lookup("file1.txt");
    assert!(result.is_err());
}
