pub mod block;
pub mod char;
pub mod console;
pub mod fb;
pub mod full;
pub mod kbd;
pub mod kmsg;
pub mod net;
pub mod null;
pub mod rtc;
pub mod serial;
pub mod urandom;
pub mod zero;

// Explicit re-exports (no globs) — public API surface is intentional.
pub use block::BlockDeviceInode;
pub use char::GenericCharDeviceInode;
pub use console::ConsoleInode;
pub use fb::FbInode;
pub use full::FullInode;
pub use kbd::KbdInode;
pub use kmsg::KmsgInode;
pub use net::NetDeviceInode;
pub use null::NullInode;
pub use rtc::RtcInode;
pub use serial::SerialInode;
pub use urandom::UrandomInode;
pub use zero::ZeroInode;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::AtomicU64;

use crate::device::{DEVICE_MANAGER, Device, DeviceType};
use crate::fs::ramfs::RamDirFileOps;
use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::mount::MOUNT_TABLE;
use crate::fs::vfs::types::{
    FileOps, FileSystem, Inode, InodeOps, InodeType, Stat, SuperBlock, VfsError,
};
use crate::sync::Mutex;

// ===== Global DevFS directory references =====

pub static DEV_ROOT_DIR: Mutex<Option<Arc<DevDirInode>>> = Mutex::new(None);
pub static DEV_PTS_DIR: Mutex<Option<Arc<DevDirInode>>> = Mutex::new(None);
pub static DEV_INPUT_DIR: Mutex<Option<Arc<DevDirInode>>> = Mutex::new(None);

// ===== DevDirInode — Directory inode for /dev, /dev/pts, /dev/input =====

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

/// Register an input device node in `/dev/input`.
pub fn register_input_node(name: &str, inode: Arc<Inode>) {
    if let Some(input_dir) = DEV_INPUT_DIR.lock().as_ref() {
        input_dir.insert(name, inode.clone());
    }
    let mt = MOUNT_TABLE.read();
    if let Some((mount, _)) = mt.lookup("/dev") {
        if let Some(input_dentry) = mount.root_dentry.children.lock().get("input").cloned() {
            Dentry::add_child(&input_dentry, name.into(), inode);
        } else {
            Dentry::add_child(&mount.root_dentry, name.into(), inode);
        }
    }
}

/// Dynamically sync a device registered in `DEVICE_MANAGER` to `/dev`.
pub fn sync_device_to_devfs(device: &Arc<Mutex<Box<dyn Device>>>) {
    if DEV_ROOT_DIR.lock().is_none() {
        return;
    }

    let mt = MOUNT_TABLE.read();
    let (mount, _) = match mt.lookup("/dev") {
        Some(m) => m,
        None => return,
    };

    let dev_lock = device.lock();
    let vfs_name = match dev_lock.dev_name() {
        Some(n) => n,
        None => return,
    };
    let dev_name = dev_lock.name();
    let dev_type = dev_lock.dev_type();
    drop(dev_lock);

    // Check if node already exists in root_dir
    if let Some(root_dir) = DEV_ROOT_DIR.lock().as_ref() {
        if root_dir.entries.lock().contains_key(vfs_name) {
            return;
        }
    }

    let ino = mount.superblock.alloc_ino();
    match dev_type {
        DeviceType::Block => {
            let inode = Arc::new(Inode {
                ino,
                inode_type: InodeType::BlockDevice,
                ops: Arc::new(BlockDeviceInode {
                    device_name: dev_name,
                }),
            });
            drop(mt);
            register_dev_node(vfs_name, inode);
            log::info!("[DevFS] Registered block device /dev/{}", vfs_name);
        }
        DeviceType::Char | DeviceType::Gpu | DeviceType::Network | DeviceType::Audio => {
            let inode = Arc::new(Inode {
                ino,
                inode_type: InodeType::CharDevice,
                ops: Arc::new(GenericCharDeviceInode {
                    device_name: dev_name,
                }),
            });
            drop(mt);
            register_dev_node(vfs_name, inode);
            log::info!("[DevFS] Registered character device /dev/{}", vfs_name);
        }
        _ => {}
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

        // Declarative list of standard UNIX core pseudo and hardware device nodes.
        let core_nodes: [DevNode; 15] = [
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
                name: "ttyS0",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(SerialInode),
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
                name: "full",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(FullInode),
            },
            DevNode {
                name: "urandom",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(UrandomInode),
            },
            DevNode {
                name: "random",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(UrandomInode),
            },
            DevNode {
                name: "kmsg",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(KmsgInode),
            },
            DevNode {
                name: "rtc0",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(RtcInode),
            },
            DevNode {
                name: "rtc",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(RtcInode),
            },
            DevNode {
                name: "kbd",
                inode_type: InodeType::CharDevice,
                ops: Arc::new(KbdInode),
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

        // Create /dev/input input subsystem directory.
        let input_dir = Arc::new(DevDirInode::new());
        *DEV_INPUT_DIR.lock() = Some(input_dir.clone());
        let input_dir_ino = dev_mount.superblock.alloc_ino();
        let input_dir_inode = Arc::new(Inode {
            ino: input_dir_ino,
            inode_type: InodeType::Directory,
            ops: input_dir.clone(),
        });
        register_node("input", input_dir_inode);

        // Register /dev/input/event0
        let event0_ino = dev_mount.superblock.alloc_ino();
        let event0_inode = Arc::new(Inode {
            ino: event0_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(KbdInode),
        });
        input_dir.insert("event0", event0_inode.clone());
        if let Some(input_dentry) = dev_mount.root_dentry.children.lock().get("input").cloned() {
            Dentry::add_child(&input_dentry, "event0".into(), event0_inode);
        }

        log::info!("[DevFS] Mounted /dev successfully with core UNIX device nodes.");
        Ok(())
    }
}

/// Scan `DEVICE_MANAGER` for all active devices and ensure their `/dev` nodes exist.
///
/// Runs as a late initcall so that all hardware drivers probed during `device_initcall`
/// are properly synchronized into `/dev`.
fn register_all_devices() -> Result<(), &'static str> {
    let dm = DEVICE_MANAGER.read();
    for dev_arc in dm.devices() {
        sync_device_to_devfs(dev_arc);
    }
    log::info!("[DevFS] All kernel devices verified and synchronized to /dev.");
    Ok(())
}

crate::late_initcall!(register_all_devices);

/// Legacy wrapper for mounting devfs (kept for compatibility).
pub fn mount_devfs() {
    let _ = DevFs::init();
}

crate::fs_initcall!(DevFs::init);
crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("Ananta98");
crate::MODULE_DESCRIPTION!("Device Filesystem Subsystem");
