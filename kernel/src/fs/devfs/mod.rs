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

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::AtomicU64;

use crate::device::DEVICE_MANAGER;
use crate::fs::ramfs::RamDirFileOps;
use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::mount::MOUNT_TABLE;
use crate::fs::vfs::types::{
    FileOps, FileSystem, Inode, InodeOps, InodeType, Stat, SuperBlock, VfsError,
};
use crate::sync::spinlock::Spinlock;

/// Directory inode for devfs (/dev and /dev/pts) to support lookup, readdir, and stat.
pub struct DevDirInode {
    pub entries: Spinlock<BTreeMap<String, Arc<Inode>>>,
}

impl DevDirInode {
    pub fn new() -> Self {
        Self {
            entries: Spinlock::new(BTreeMap::new()),
        }
    }

    pub fn insert(&self, name: &str, inode: Arc<Inode>) {
        self.entries.lock().insert(name.into(), inode);
    }
}

impl InodeOps for DevDirInode {
    fn lookup(&self, name: &str) -> Result<Arc<Inode>, VfsError> {
        let entries = self.entries.lock();
        entries.get(name).cloned().ok_or(VfsError::NotFound)
    }

    fn readdir(&self) -> Result<Vec<String>, VfsError> {
        let entries = self.entries.lock();
        Ok(entries.keys().cloned().collect())
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        let entries = self.entries.lock();
        Ok(Stat {
            size: entries.len() as u64,
            mode: 0o040755, // S_IFDIR | 0755
            nlink: 2,
            ..Default::default()
        })
    }

    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(RamDirFileOps))
    }
}

pub static DEV_ROOT_DIR: Spinlock<Option<Arc<DevDirInode>>> = Spinlock::new(None);
pub static DEV_PTS_DIR: Spinlock<Option<Arc<DevDirInode>>> = Spinlock::new(None);

/// Register a device node dynamically in `/dev`.
pub fn register_dev_node(name: &str, inode: Arc<Inode>) {
    if let Some(root_dir) = DEV_ROOT_DIR.lock().as_ref() {
        root_dir.insert(name, inode.clone());
    }
    let mt = MOUNT_TABLE.read();
    if let Some((mount, _)) = mt.lookup("/dev") {
        Dentry::add_child(&mount.root_dentry, name.into(), inode);
    }
}

/// Register a pseudo-terminal slave device node in `/dev/pts`.
pub fn register_pts_node(name: &str, inode: Arc<Inode>) {
    if let Some(pts_dir) = DEV_PTS_DIR.lock().as_ref() {
        pts_dir.insert(name, inode.clone());
    }
    let mt = MOUNT_TABLE.read();
    if let Some((mount, _)) = mt.lookup("/dev") {
        if let Some(pts_dentry) = mount.root_dentry.children.lock().get("pts").cloned() {
            Dentry::add_child(&pts_dentry, name.into(), inode);
        } else {
            Dentry::add_child(&mount.root_dentry, name.into(), inode);
        }
    }
}

/// Device filesystem, mounted at `/dev`.
pub struct DevFs;

impl FileSystem for DevFs {
    fn name(&self) -> &'static str {
        "devfs"
    }

    fn mount(&self) -> Result<SuperBlock, VfsError> {
        let next_ino = AtomicU64::new(1);
        let ino = next_ino.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        let root_dir = Arc::new(DevDirInode::new());
        *DEV_ROOT_DIR.lock() = Some(root_dir.clone());

        let root_inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::Directory,
            ops: root_dir,
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

        let root_dir = DEV_ROOT_DIR
            .lock()
            .clone()
            .ok_or("DevFs root dir uninitialized")?;

        let register_node = |name: &str, inode: Arc<Inode>| {
            root_dir.insert(name, inode.clone());
            Dentry::add_child(&dev_mount.root_dentry, name.into(), inode);
        };

        // Register console character device (/dev/console, /dev/tty, /dev/tty0)
        let console_ino = dev_mount.superblock.alloc_ino();
        let console_inode = Arc::new(Inode {
            ino: console_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(ConsoleInode),
        });
        register_node("console", console_inode.clone());
        register_node("tty", console_inode.clone());
        register_node("tty0", console_inode);

        // Register framebuffer device node /dev/fb0
        let fb_ino = dev_mount.superblock.alloc_ino();
        let fb_inode = Arc::new(Inode {
            ino: fb_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(FbInode),
        });
        register_node("fb0", fb_inode);

        // Register pseudo-terminal multiplexer /dev/ptmx
        let ptmx_ino = dev_mount.superblock.alloc_ino();
        let ptmx_inode = Arc::new(Inode {
            ino: ptmx_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(crate::tty::pty::PtmxInode),
        });
        register_node("ptmx", ptmx_inode);

        // Create /dev/pts pseudo-terminal slave directory
        let pts_dir = Arc::new(DevDirInode::new());
        *DEV_PTS_DIR.lock() = Some(pts_dir.clone());
        let pts_dir_ino = dev_mount.superblock.alloc_ino();
        let pts_dir_inode = Arc::new(Inode {
            ino: pts_dir_ino,
            inode_type: InodeType::Directory,
            ops: pts_dir,
        });
        register_node("pts", pts_dir_inode);

        // Register null character device /dev/null
        let null_ino = dev_mount.superblock.alloc_ino();
        let null_inode = Arc::new(Inode {
            ino: null_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(NullInode),
        });
        register_node("null", null_inode);

        // Register zero character device /dev/zero
        let zero_ino = dev_mount.superblock.alloc_ino();
        let zero_inode = Arc::new(Inode {
            ino: zero_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(ZeroInode),
        });
        register_node("zero", zero_inode);

        // Register urandom character device /dev/urandom
        let urandom_ino = dev_mount.superblock.alloc_ino();
        let urandom_inode = Arc::new(Inode {
            ino: urandom_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(UrandomInode),
        });
        register_node("urandom", urandom_inode);

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
                register_node(vfs_name, block_inode);
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
crate::MODULE_AUTHOR!("Ananta98");
crate::MODULE_DESCRIPTION!("Device Filesystem Subsystem");
