pub mod block;
pub mod console;
pub mod fb;
pub mod null;
pub mod urandom;
pub mod zero;

// Explicit re-exports (no globs) — public API surface is intentional.
pub use block::BlockDeviceInode;
pub use console::ConsoleInode;
pub use fb::FbInode;
pub use null::NullInode;
pub use urandom::UrandomInode;
pub use zero::ZeroInode;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::AtomicU64;

use crate::device::{DeviceType, DEVICE_MANAGER};
use crate::fs::ramfs::RamDirFileOps;
use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::mount::MOUNT_TABLE;
use crate::fs::vfs::types::{
    FileOps, FileSystem, Inode, InodeOps, InodeType, Stat, SuperBlock, VfsError,
};
use crate::sync::Mutex;

// ===== DevDirInode — Directory inode for /dev and /dev/pts =====

/// Directory inode for devfs that supports dynamic device node insertion,
/// lookup, readdir, and stat.
pub struct DevDirInode {
    pub entries: Mutex<BTreeMap<String, Arc<Inode>>>,
}

impl DevDirInode {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(BTreeMap::new()),
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

// ===== Global DevFS directory references =====

pub static DEV_ROOT_DIR: Mutex<Option<Arc<DevDirInode>>> = Mutex::new(None);
pub static DEV_PTS_DIR: Mutex<Option<Arc<DevDirInode>>> = Mutex::new(None);

// ===== Dynamic device node registration =====

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

// ===== DevNode descriptor =====

/// Descriptor for a statically-defined device node to be registered in `/dev`.
struct DevNode {
    name: &'static str,
    inode_type: InodeType,
    ops: Arc<dyn InodeOps>,
}

// ===== DevFs =====

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
    /// Mount the device filesystem at `/dev` and register all core device nodes.
    pub fn init() -> Result<(), &'static str> {
        let mut mt = MOUNT_TABLE.write();
        let dev_mount = mt
            .mount("/dev", &DevFs)
            .map_err(|_| "Failed to mount devfs at /dev")?;

        let root_dir = DEV_ROOT_DIR
            .lock()
            .clone()
            .ok_or("DevFs root dir uninitialized")?;

        // Helper closure: inserts the node into both the dir inode and the dentry tree.
        let register_node = |name: &str, inode: Arc<Inode>| {
            root_dir.insert(name, inode.clone());
            Dentry::add_child(&dev_mount.root_dentry, name.into(), inode);
        };

        // Declarative list of static core device nodes.
        let core_nodes: [DevNode; 8] = [
            DevNode {
                name: "console",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(ConsoleInode),
            },
            DevNode {
                name: "tty",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(ConsoleInode),
            },
            DevNode {
                name: "tty0",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(ConsoleInode),
            },
            DevNode {
                name: "fb0",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(FbInode),
            },
            DevNode {
                name: "ptmx",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(crate::tty::pty::PtmxInode),
            },
            DevNode {
                name: "null",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(NullInode),
            },
            DevNode {
                name: "zero",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(ZeroInode),
            },
            DevNode {
                name: "urandom",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(UrandomInode),
            },
        ];

        for node in &core_nodes {
            let ino = dev_mount.superblock.alloc_ino();
            let inode = Arc::new(Inode {
                ino,
                inode_type: node.inode_type,
                ops: node.ops.clone(),
            });
            register_node(node.name, inode);
        }

        // Create /dev/pts pseudo-terminal slave directory.
        let pts_dir = Arc::new(DevDirInode::new());
        *DEV_PTS_DIR.lock() = Some(pts_dir.clone());
        let pts_dir_ino = dev_mount.superblock.alloc_ino();
        let pts_dir_inode = Arc::new(Inode {
            ino: pts_dir_ino,
            inode_type: InodeType::Directory,
            ops: pts_dir,
        });
        register_node("pts", pts_dir_inode);

        // Register discovered block devices from DEVICE_MANAGER.
        drop(mt);
        Self::register_block_devices(&dev_mount.superblock, register_node);

        log::info!("[DevFS] Mounted /dev successfully.");
        Ok(())
    }

    /// Scan DEVICE_MANAGER for block devices and register them as /dev nodes.
    fn register_block_devices<F>(superblock: &SuperBlock, mut register_node: F)
    where
        F: FnMut(&str, Arc<Inode>),
    {
        let dm = DEVICE_MANAGER.read();
        for dev_arc in dm.get_by_type(DeviceType::Block) {
            let dev_lock = dev_arc.lock();
            let dev_name = dev_lock.name();
            let vfs_name = match dev_lock.dev_name() {
                Some(name) => name,
                None => continue,
            };

            let block_ino = superblock.alloc_ino();
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
}

/// Legacy wrapper for mounting devfs (kept for compatibility).
pub fn mount_devfs() {
    let _ = DevFs::init();
}

crate::fs_initcall!(DevFs::init);
crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("Ananta98");
crate::MODULE_DESCRIPTION!("Device Filesystem Subsystem");
