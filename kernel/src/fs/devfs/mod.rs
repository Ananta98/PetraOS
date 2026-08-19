pub mod block;
pub mod console;
pub mod fb;
pub mod null;
pub mod urandom;
pub mod zero;

pub use block::*;
pub use console::*;
pub use fb::*;
pub use null::*;
pub use urandom::*;
pub use zero::*;

use alloc::sync::Arc;
use core::sync::atomic::AtomicU64;

use crate::device::DEVICE_MANAGER;
use crate::fs::ramfs::RamDirInode;
use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::mount::MOUNT_TABLE;
use crate::fs::vfs::types::{FileSystem, Inode, InodeType, SuperBlock, VfsError};

/// Device filesystem, mounted at `/dev`.
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

impl DevFs {
    /// Mount the device filesystem at `/dev` and register core device nodes.
    pub fn init() -> Result<(), &'static str> {
        let mut mt = MOUNT_TABLE.write();
        let dev_mount = mt
            .mount("/dev", &DevFs)
            .map_err(|_| "Failed to mount devfs at /dev")?;

        // Register console character device (/dev/console, /dev/tty, /dev/tty0)
        let console_ino = dev_mount.superblock.alloc_ino();
        let console_inode = Arc::new(Inode {
            ino: console_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(ConsoleInode),
        });
        Dentry::add_child(&dev_mount.root_dentry, "console".into(), console_inode.clone());
        Dentry::add_child(&dev_mount.root_dentry, "tty".into(), console_inode.clone());
        Dentry::add_child(&dev_mount.root_dentry, "tty0".into(), console_inode);

        // Register framebuffer device node /dev/fb0
        let fb_ino = dev_mount.superblock.alloc_ino();
        let fb_inode = Arc::new(Inode {
            ino: fb_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(FbInode),
        });
        Dentry::add_child(&dev_mount.root_dentry, "fb0".into(), fb_inode);

        // Register pseudo-terminal multiplexer /dev/ptmx
        let ptmx_ino = dev_mount.superblock.alloc_ino();
        let ptmx_inode = Arc::new(Inode {
            ino: ptmx_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(crate::tty::pty::PtmxInode),
        });
        Dentry::add_child(&dev_mount.root_dentry, "ptmx".into(), ptmx_inode);

        // Create /dev/pts pseudo-terminal slave directory
        let pts_dir_ino = dev_mount.superblock.alloc_ino();
        let pts_dir_inode = Arc::new(Inode {
            ino: pts_dir_ino,
            inode_type: InodeType::Directory,
            ops: Arc::new(RamDirInode::new()),
        });
        Dentry::add_child(&dev_mount.root_dentry, "pts".into(), pts_dir_inode);

        // Register null character device /dev/null
        let null_ino = dev_mount.superblock.alloc_ino();
        let null_inode = Arc::new(Inode {
            ino: null_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(NullInode),
        });
        Dentry::add_child(&dev_mount.root_dentry, "null".into(), null_inode);

        // Register zero character device /dev/zero
        let zero_ino = dev_mount.superblock.alloc_ino();
        let zero_inode = Arc::new(Inode {
            ino: zero_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(ZeroInode),
        });
        Dentry::add_child(&dev_mount.root_dentry, "zero".into(), zero_inode);

        // Register urandom character device /dev/urandom
        let urandom_ino = dev_mount.superblock.alloc_ino();
        let urandom_inode = Arc::new(Inode {
            ino: urandom_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(UrandomInode),
        });
        Dentry::add_child(&dev_mount.root_dentry, "urandom".into(), urandom_inode);

        // Scan DEVICE_MANAGER and dynamically register discovered block devices
        let dm = DEVICE_MANAGER.read();
        for dev_arc in dm.get_devices() {
            let dev_lock = dev_arc.lock();
            if dev_lock.dev_type() == crate::device::DeviceType::Block {
                let dev_name = dev_lock.name();
                let vfs_name = if dev_name.contains("AHCI") {
                    "sda"
                } else if dev_name.contains("NVMe") {
                    "nvme0n1"
                } else {
                    continue;
                };

                let block_ino = dev_mount.superblock.alloc_ino();
                let block_inode = Arc::new(Inode {
                    ino: block_ino,
                    inode_type: InodeType::BlockDevice,
                    ops: Arc::new(BlockDeviceInode {
                        device_name: dev_name,
                    }),
                });
                Dentry::add_child(&dev_mount.root_dentry, vfs_name.into(), block_inode);
            }
        }

        log::info!("[DevFS] Mounted /dev successfully.");
        Ok(())
    }
}

/// Legacy wrapper for mounting devfs.
pub fn mount_devfs() {
    let _ = DevFs::init();
}

crate::fs_initcall!(DevFs::init);
crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("PetraOS Development Team");
crate::MODULE_DESCRIPTION!("Device Filesystem Subsystem");
