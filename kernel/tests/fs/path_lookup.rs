extern crate alloc;

// Tests for MountTable lookup and basic mount operations.

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
        #[path = "../../src/fs/vfs/mount.rs"]
        pub mod mount;
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
use fs::vfs::filesystem::{FileSystem, SuperBlock};
use fs::vfs::inode::{Inode, InodeOps, InodeType};
use fs::vfs::mount::MountTable;
use fs::ramfs::inode::RamDirInode;
use std::sync::Arc;
use core::sync::atomic::AtomicU64;

/// A simple test filesystem that produces a ramfs root.
struct TestFs {
    name: &'static str,
}

impl FileSystem for TestFs {
    fn name(&self) -> &'static str { self.name }

    fn mount(&self) -> Result<SuperBlock, VfsError> {
        let next_ino = AtomicU64::new(1);
        let ino = next_ino.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let root_inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::Directory,
            ops: Arc::new(RamDirInode::new()),
        });
        Ok(SuperBlock { fs_name: self.name, root_inode, next_ino, read_only: false })
    }
}

#[test]
fn test_mount_root() {
    let mut mt = MountTable::new();
    let result = mt.mount("/", &TestFs { name: "rootfs" });
    assert!(result.is_ok());
    assert!(mt.root().is_some());
}

#[test]
fn test_lookup_root_exact() {
    let mut mt = MountTable::new();
    mt.mount("/", &TestFs { name: "rootfs" }).unwrap();

    let result = mt.lookup("/");
    assert!(result.is_some());
    let (mount, remainder) = result.unwrap();
    assert_eq!(mount.mount_point, "/");
    assert_eq!(remainder, "");
}

#[test]
fn test_lookup_path_under_root() {
    let mut mt = MountTable::new();
    mt.mount("/", &TestFs { name: "rootfs" }).unwrap();

    let result = mt.lookup("/hello.txt");
    assert!(result.is_some());
    let (mount, remainder) = result.unwrap();
    assert_eq!(mount.mount_point, "/");
    assert_eq!(remainder, "hello.txt");
}

#[test]
fn test_lookup_dev_mount() {
    let mut mt = MountTable::new();
    mt.mount("/", &TestFs { name: "rootfs" }).unwrap();
    mt.mount("/dev", &TestFs { name: "devfs" }).unwrap();

    let result = mt.lookup("/dev/console");
    assert!(result.is_some());
    let (mount, remainder) = result.unwrap();
    assert_eq!(mount.mount_point, "/dev");
    assert_eq!(remainder, "console");
}

#[test]
fn test_lookup_dev_exact() {
    let mut mt = MountTable::new();
    mt.mount("/", &TestFs { name: "rootfs" }).unwrap();
    mt.mount("/dev", &TestFs { name: "devfs" }).unwrap();

    let result = mt.lookup("/dev");
    assert!(result.is_some());
    let (mount, remainder) = result.unwrap();
    assert_eq!(mount.mount_point, "/dev");
    assert_eq!(remainder, "");
}

#[test]
fn test_lookup_longest_prefix() {
    let mut mt = MountTable::new();
    mt.mount("/", &TestFs { name: "rootfs" }).unwrap();
    mt.mount("/dev", &TestFs { name: "devfs" }).unwrap();
    mt.mount("/tmp", &TestFs { name: "tmpfs" }).unwrap();

    // /tmp/file should match /tmp mount
    let result = mt.lookup("/tmp/file");
    assert!(result.is_some());
    let (mount, remainder) = result.unwrap();
    assert_eq!(mount.mount_point, "/tmp");
    assert_eq!(remainder, "file");

    // /other should match / mount
    let result2 = mt.lookup("/other");
    assert!(result2.is_some());
    let (mount2, remainder2) = result2.unwrap();
    assert_eq!(mount2.mount_point, "/");
    assert_eq!(remainder2, "other");
}

#[test]
fn test_lookup_no_false_prefix_match() {
    let mut mt = MountTable::new();
    mt.mount("/", &TestFs { name: "rootfs" }).unwrap();
    mt.mount("/dev", &TestFs { name: "devfs" }).unwrap();

    // "/developer" should NOT match "/dev" mount — it should fall through to "/"
    let result = mt.lookup("/developer");
    assert!(result.is_some());
    let (mount, remainder) = result.unwrap();
    assert_eq!(mount.mount_point, "/");
    assert_eq!(remainder, "developer");
}
