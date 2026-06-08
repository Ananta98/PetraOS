extern crate alloc;

// Tests for devfs console device.

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
    pub mod devfs {
        #[path = "../../src/fs/devfs/console.rs"]
        pub mod console;
    }
}

use fs::vfs::inode::InodeOps;
use fs::vfs::file::FileOps;
use fs::devfs::console::ConsoleInode;

#[test]
fn test_console_open_succeeds() {
    let console = ConsoleInode;
    let ops = console.open();
    assert!(ops.is_ok());
}

#[test]
fn test_console_write_returns_byte_count() {
    let console = ConsoleInode;
    let ops = console.open().expect("open should succeed");
    let data = b"Hello from console test";
    let result = ops.write(0, data);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), data.len());
}

#[test]
fn test_console_read_returns_zero() {
    let console = ConsoleInode;
    let ops = console.open().expect("open should succeed");
    let mut buf = [0u8; 16];
    let result = ops.read(0, &mut buf);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}
