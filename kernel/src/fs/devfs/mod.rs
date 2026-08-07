pub mod block;
pub mod console;

use crate::fs::devfs::block::BlockDeviceInode;
use crate::fs::devfs::console::ConsoleInode;
use crate::fs::errno::VfsError;
use crate::fs::ramfs::inode::RamDirInode;
use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::filesystem::{FileSystem, SuperBlock};
use crate::fs::vfs::inode::{Inode, InodeType};
use crate::fs::vfs::mount::MOUNT_TABLE;
use alloc::sync::Arc;
use core::sync::atomic::AtomicU64;

/// Device filesystem, mounted at `/dev`.
///
/// Uses a `RamDirInode` as its root directory and exposes device inodes
/// registered via [`DevFs::register_device`].
pub struct DevFs;

impl FileSystem for DevFs {
    fn name(&self) -> &'static str {
        "devfs"
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
            fs_name: "devfs",
            root_inode,
            next_ino,
            read_only: false,
        })
    }
}

/// Mount the device filesystem at `/dev` and register core device nodes.
pub fn mount_devfs() {
    let mut mt = MOUNT_TABLE.lock();
    let dev_mount = mt
        .mount("/dev", &DevFs)
        .expect("Failed to mount devfs at /dev");

    // Register console character device
    let console_ino = dev_mount.superblock.alloc_ino();
    let console_inode = Arc::new(crate::fs::vfs::inode::Inode {
        ino: console_ino,
        inode_type: crate::fs::vfs::inode::InodeType::CharDevice,
        ops: Arc::new(ConsoleInode),
    });
    Dentry::add_child(&dev_mount.root_dentry, "console".into(), console_inode);

    // Scan DEVICE_MANAGER and dynamically register discovered block devices
    let dm = crate::drivers::DEVICE_MANAGER.lock();
    for dev_arc in dm.get_devices() {
        let dev_lock = dev_arc.lock();
        if dev_lock.dev_type() == crate::drivers::DeviceType::Block {
            let dev_name = dev_lock.name();
            let vfs_name = if dev_name.contains("AHCI") {
                "sda"
            } else if dev_name.contains("NVMe") {
                "nvme0n1"
            } else {
                continue;
            };

            let block_ino = dev_mount.superblock.alloc_ino();
            let block_inode = Arc::new(crate::fs::vfs::inode::Inode {
                ino: block_ino,
                inode_type: crate::fs::vfs::inode::InodeType::BlockDevice,
                ops: Arc::new(BlockDeviceInode {
                    device_name: dev_name,
                }),
            });
            Dentry::add_child(&dev_mount.root_dentry, vfs_name.into(), block_inode);
        }
    }
}
