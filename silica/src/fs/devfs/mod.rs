// SPDX-License-Identifier: GPL-2.0

//! Device Filesystem (`/dev`).
//!
//! Provides a dynamic, extensible character and block device node registry
//! for PetraOS drivers to register and unregister device nodes at runtime.

pub mod blk;
pub mod chr;
pub mod drm;
pub mod input;
pub mod net;
pub mod null;
pub mod random;
pub mod zero;

pub use null::NullFile;
pub use random::{RandomFile, UrandomFile};
pub use zero::ZeroFile;

use crate::fs::vfs::{
    LookupFlags,
    dcache::DEntry,
    lookup,
    mount::{Mount, PathNode, root},
};
use crate::{
    uapi::errno::{Errno, Result},
    fs::vfs::{FileOps, FileSystem, INode, INodeAttr, Mode, NodeOps, OpenFlags, StatFs},
};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use spin::RwLock;
use blk::GenericBlkNodeOps;
use chr::GenericChrNodeOps;
use null::NullNodeOps;
use random::{RandomNodeOps, UrandomNodeOps};
use zero::ZeroNodeOps;

/// Operations on the `/dev` root directory inode.
pub struct DevFsDirOps {
    pub(crate) dynamic_entries: Arc<RwLock<BTreeMap<String, Arc<INode>>>>,
}

impl DevFsDirOps {
    pub fn new() -> Self {
        Self {
            dynamic_entries: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub fn with_entries(dynamic_entries: Arc<RwLock<BTreeMap<String, Arc<INode>>>>) -> Self {
        Self { dynamic_entries }
    }
}

impl Default for DevFsDirOps {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeOps for DevFsDirOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        // 1. Check dynamic dev entries first (created directories, symlinks, files)
        if let Some(node) = self.dynamic_entries.read().get(name) {
            return Ok(Arc::clone(node));
        }

        // 2. Check static dev nodes
        match name {
            "null" => {
                return Ok(Arc::new(INode::new(
                    Box::new(NullNodeOps),
                    Mode::CHAR
                        | Mode::RUSR
                        | Mode::WUSR
                        | Mode::RGRP
                        | Mode::WGRP
                        | Mode::ROTH
                        | Mode::WOTH,
                    1,
                )));
            }
            "zero" => {
                return Ok(Arc::new(INode::new(
                    Box::new(ZeroNodeOps),
                    Mode::CHAR
                        | Mode::RUSR
                        | Mode::WUSR
                        | Mode::RGRP
                        | Mode::WGRP
                        | Mode::ROTH
                        | Mode::WOTH,
                    2,
                )));
            }
            "random" => {
                return Ok(Arc::new(INode::new(
                    Box::new(RandomNodeOps),
                    Mode::CHAR
                        | Mode::RUSR
                        | Mode::WUSR
                        | Mode::RGRP
                        | Mode::WGRP
                        | Mode::ROTH
                        | Mode::WOTH,
                    3,
                )));
            }
            "urandom" => {
                return Ok(Arc::new(INode::new(
                    Box::new(UrandomNodeOps),
                    Mode::CHAR
                        | Mode::RUSR
                        | Mode::WUSR
                        | Mode::RGRP
                        | Mode::WGRP
                        | Mode::ROTH
                        | Mode::WOTH,
                    4,
                )));
            }
            "tty" => {
                return Ok(Arc::new(INode::new(
                    Box::new(TtyNodeOps),
                    Mode::CHAR
                        | Mode::RUSR
                        | Mode::WUSR
                        | Mode::RGRP
                        | Mode::WGRP
                        | Mode::ROTH
                        | Mode::WOTH,
                    5,
                )));
            }
            "ptmx" => {
                return Ok(Arc::new(INode::new(
                    Box::new(crate::drivers::char::tty::pty::PtmxNodeOps),
                    Mode::CHAR
                        | Mode::RUSR
                        | Mode::WUSR
                        | Mode::RGRP
                        | Mode::WGRP
                        | Mode::ROTH
                        | Mode::WOTH,
                    10,
                )));
            }
            "pts" => {
                return Ok(Arc::new(INode::new(
                    Box::new(crate::drivers::char::tty::pty::PtsDirNodeOps),
                    Mode::DIR
                        | Mode::RUSR
                        | Mode::WUSR
                        | Mode::XUSR
                        | Mode::RGRP
                        | Mode::XGRP
                        | Mode::ROTH
                        | Mode::XOTH,
                    11,
                )));
            }
            "dri" => {
                return Ok(Arc::new(INode::new(
                    Box::new(drm::DriDirNodeOps),
                    Mode::DIR
                        | Mode::RUSR
                        | Mode::WUSR
                        | Mode::XUSR
                        | Mode::RGRP
                        | Mode::XGRP
                        | Mode::ROTH
                        | Mode::XOTH,
                    12,
                )));
            }
            "input" => {
                return Ok(Arc::new(INode::new(
                    Box::new(input::InputDirNodeOps),
                    Mode::DIR
                        | Mode::RUSR
                        | Mode::WUSR
                        | Mode::XUSR
                        | Mode::RGRP
                        | Mode::XGRP
                        | Mode::ROTH
                        | Mode::XOTH,
                    13,
                )));
            }
            _ => {}
        }

        // 3. Check character devices
        for dev in crate::drivers::get_chrdevs() {
            if dev.name() == name {
                return Ok(Arc::new(INode::new(
                    Box::new(GenericChrNodeOps::new(Arc::clone(&dev))),
                    Mode::CHAR
                        | Mode::RUSR
                        | Mode::WUSR
                        | Mode::RGRP
                        | Mode::WGRP
                        | Mode::ROTH
                        | Mode::WOTH,
                    100, // In real implementations, you'd use a unique inode number
                )));
            }
        }

        // 4. Check block devices
        for dev in crate::drivers::get_blkdevs() {
            if dev.name() == name {
                return Ok(Arc::new(INode::new(
                    Box::new(GenericBlkNodeOps::new(Arc::clone(&dev))),
                    Mode::BLOCK | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP,
                    100, // In real implementations, you'd use a unique inode number
                )));
            }
        }

        // 5. Check network devices
        for dev in crate::drivers::get_netdevs() {
            if dev.name() == name {
                return Ok(Arc::new(INode::new(
                    Box::new(net::GenericNetNodeOps::new(Arc::clone(&dev))),
                    Mode::CHAR | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH,
                    100,
                )));
            }
        }

        crate::return_errno!(ENOENT, "device not found")
    }

    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EPERM, "cannot open /dev directory for writing");
        }
        Ok(Box::new(DevFsDirFileOps {
            dynamic_entries: Arc::clone(&self.dynamic_entries),
        }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::DIR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::XUSR
                | Mode::RGRP
                | Mode::WGRP
                | Mode::XGRP
                | Mode::ROTH
                | Mode::WOTH
                | Mode::XOTH,
            nlink: 2,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }

    fn mkdir(&self, name: &str, mode: Mode) -> Result<Arc<INode>> {
        static NEXT_INO: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(100_000);
        let ino = NEXT_INO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let dir_entries = Arc::new(RwLock::new(BTreeMap::new()));
        let node = Arc::new(INode::new(
            Box::new(crate::fs::initramfs::InitramfsDirNodeOps {
                entries: dir_entries,
            }),
            mode | Mode::DIR,
            ino,
        ));
        self.dynamic_entries
            .write()
            .insert(String::from(name), node.clone());
        Ok(node)
    }

    fn symlink(&self, name: &str, target: &str) -> Result<Arc<INode>> {
        static NEXT_INO: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(100_000);
        let ino = NEXT_INO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let node = Arc::new(INode::new_with_attr(
            Box::new(DevSymlinkNodeOps {
                target: String::from(target),
            }),
            ino,
            INodeAttr {
                size: target.len(),
                mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                nlink: 1,
                uid: 0,
                gid: 0,
                rdev: 0,
            },
        ));
        self.dynamic_entries
            .write()
            .insert(String::from(name), node.clone());
        Ok(node)
    }

    fn unlink(&self, name: &str) -> Result<()> {
        if self.dynamic_entries.write().remove(name).is_some() {
            return Ok(());
        }
        crate::return_errno!(ENOENT, "file not found in /dev")
    }

    fn rmdir(&self, name: &str) -> Result<()> {
        let mut entries = self.dynamic_entries.write();
        let child = entries.get(name).ok_or(Errno::ENOENT)?;
        if !child.is_dir() {
            return Err(Errno::ENOTDIR);
        }
        entries.remove(name);
        Ok(())
    }

    fn create(&self, name: &str, mode: Mode) -> Result<Arc<INode>> {
        static NEXT_INO: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(100_000);
        let ino = NEXT_INO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let data = Arc::new(RwLock::new(Vec::new()));
        let attr = INodeAttr {
            size: 0,
            mode,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        };
        let node = Arc::new(INode::new_with_attr(
            Box::new(crate::fs::initramfs::InitramfsRamFileNodeOps::new_with_attr(data, attr)),
            ino,
            attr,
        ));
        self.dynamic_entries
            .write()
            .insert(String::from(name), node.clone());
        Ok(node)
    }

    fn link(&self, name: &str, other: Arc<INode>) -> Result<()> {
        self.dynamic_entries
            .write()
            .insert(String::from(name), other);
        Ok(())
    }

    fn mknod(&self, name: &str, mode: Mode, rdev: u64) -> Result<Arc<INode>> {
        static NEXT_INO: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(100_000);
        let ino = NEXT_INO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let attr = INodeAttr {
            size: 0,
            mode,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev,
        };
        let node = Arc::new(INode::new_with_attr(
            Box::new(DevSpecialNodeOps),
            ino,
            attr,
        ));
        self.dynamic_entries
            .write()
            .insert(String::from(name), node.clone());
        Ok(node)
    }

    fn rename(&self, old_name: &str, new_dir: &Arc<INode>, new_name: &str) -> Result<()> {
        let node = self.dynamic_entries.write().remove(old_name).ok_or(Errno::ENOENT)?;
        if let Err(e) = new_dir.node_ops.link(new_name, node.clone()) {
            self.dynamic_entries.write().insert(String::from(old_name), node);
            return Err(e);
        }
        Ok(())
    }
}

/// Directory file operations for reading `/dev`.
pub struct DevFsDirFileOps {
    pub(crate) dynamic_entries: Arc<RwLock<BTreeMap<String, Arc<INode>>>>,
}

impl FileOps for DevFsDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(b".\n..\n");
        out.extend_from_slice(b"null\n");
        out.extend_from_slice(b"zero\n");
        out.extend_from_slice(b"random\n");
        out.extend_from_slice(b"urandom\n");
        out.extend_from_slice(b"tty\n");
        out.extend_from_slice(b"ptmx\n");
        out.extend_from_slice(b"pts\n");
        out.extend_from_slice(b"dri\n");
        out.extend_from_slice(b"input\n");

        for key in self.dynamic_entries.read().keys() {
            out.extend_from_slice(key.as_bytes());
            out.push(b'\n');
        }

        for dev in crate::drivers::get_chrdevs() {
            out.extend_from_slice(dev.name().as_bytes());
            out.push(b'\n');
        }

        for dev in crate::drivers::get_blkdevs() {
            out.extend_from_slice(dev.name().as_bytes());
            out.push(b'\n');
        }

        for dev in crate::drivers::get_netdevs() {
            out.extend_from_slice(dev.name().as_bytes());
            out.push(b'\n');
        }

        Ok(out)
    }
}

/// Special device node operations created via `mknod` in `/dev`.
pub struct DevSpecialNodeOps;

impl NodeOps for DevSpecialNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        crate::return_errno!(ENODEV, "device not configured");
    }
}

/// Symbolic link node operations in `/dev`.
pub struct DevSymlinkNodeOps {
    pub target: String,
}

impl NodeOps for DevSymlinkNodeOps {
    fn readlink(&self) -> Result<String> {
        Ok(self.target.clone())
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: self.target.len(),
            mode: Mode::LINK | Mode::from_bits_truncate(0o777),
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

/// Controlling terminal (`/dev/tty`) node operations.
pub struct TtyNodeOps;

impl NodeOps for TtyNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        let tty = crate::drivers::char::tty::get_console().ok_or(Errno::ENODEV)?;
        Ok(Box::new(crate::drivers::char::tty::TtyFileOps { tty }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::CHAR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::RGRP
                | Mode::WGRP
                | Mode::ROTH
                | Mode::WOTH,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0x0500, // makedev(5, 0)
        })
    }
}

/// The DevFS filesystem representation.
pub struct DevFs {
    root: Arc<INode>,
}

impl DevFs {
    pub fn new() -> Self {
        let entries = Arc::new(RwLock::new(BTreeMap::new()));

        let default_symlinks = [
            ("fd", "/proc/self/fd"),
            ("stdin", "/proc/self/fd/0"),
            ("stdout", "/proc/self/fd/1"),
            ("stderr", "/proc/self/fd/2"),
        ];
        static NEXT_INO: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(6);
        for (name, target) in default_symlinks {
            let ino = NEXT_INO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            let node = Arc::new(INode::new_with_attr(
                Box::new(DevSymlinkNodeOps {
                    target: String::from(target),
                }),
                ino,
                INodeAttr {
                    size: target.len(),
                    mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ));
            entries.write().insert(String::from(name), node);
        }

        let dir_ops = Box::new(DevFsDirOps {
            dynamic_entries: entries,
        });
        let root = Arc::new(INode::new(
            dir_ops,
            Mode::DIR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::XUSR
                | Mode::RGRP
                | Mode::WGRP
                | Mode::XGRP
                | Mode::ROTH
                | Mode::WOTH
                | Mode::XOTH,
            0,
        ));
        Self { root }
    }
}

impl Default for DevFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for DevFs {
    fn name(&self) -> &'static str {
        "devfs"
    }

    fn root(&self) -> Result<Arc<INode>> {
        Ok(Arc::clone(&self.root))
    }

    fn statfs(&self) -> Result<StatFs> {
        Ok(StatFs {
            bsize: 4096,
            blocks: 0,
            bfree: 0,
            bavail: 0,
            files: 0,
            ffree: 0,
            namelen: 255,
        })
    }
}

/// Initializes DevFS and mounts it onto `/dev`.
pub fn init() -> Result<()> {
    let devfs = Arc::new(DevFs::new());
    let dev_inode = devfs.root()?;
    let root_node = root()?;

    let target_node = match lookup(
        root_node.clone(),
        root_node.clone(),
        "/dev",
        LookupFlags::DIRECTORY,
    ) {
        Ok(node) => node,
        Err(_) => {
            let dev_node_ops = Box::new(crate::fs::initramfs::InitramfsDirNodeOps::new());
            let dev_dir_inode = Arc::new(INode::new(
                dev_node_ops,
                Mode::DIR | Mode::RUSR | Mode::WUSR | Mode::XUSR | Mode::RGRP | Mode::ROTH,
                998,
            ));
            let dev_dentry = Arc::new(DEntry::new(
                String::from("dev"),
                Some(dev_dir_inode),
                Some(Arc::downgrade(&root_node.dentry)),
            ));
            root_node.dentry.add_child(dev_dentry.clone());
            PathNode {
                mount: root_node.mount.clone(),
                dentry: dev_dentry,
            }
        }
    };

    let mount_dentry = Arc::new(DEntry::new(
        String::from("dev"),
        Some(dev_inode),
        Some(Arc::downgrade(&target_node.dentry)),
    ));
    let dev_mount = Arc::new(Mount::new(Arc::clone(&mount_dentry), devfs));
    target_node.mount(dev_mount)?;

    ostd::info!("VFS: Mounted devfs on /dev");
    Ok(())
}

// ----------------------------------------------------------------------------
// Kernel Module Declaration via module! macro
// ----------------------------------------------------------------------------

crate::module!(
    "Device Filesystem",
    "PetraOS Team",
    crate::modules::InitcallLevel::Fs,
    init
);

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn devfs_mkdir_and_symlink_operations() {
        let devfs = DevFs::new();
        let root = devfs.root().expect("devfs root");

        // Verify default symlinks exist
        let fd_node = root.node_ops.lookup("fd").expect("lookup fd");
        assert_eq!(fd_node.node_ops.readlink().expect("readlink fd"), "/proc/self/fd");

        // Test mkdir
        let shm_node = root
            .node_ops
            .mkdir("shm", Mode::DIR | Mode::RUSR | Mode::WUSR | Mode::XUSR)
            .expect("mkdir shm");
        assert!(shm_node.is_dir());

        let lookup_shm = root.node_ops.lookup("shm").expect("lookup shm");
        assert!(lookup_shm.is_dir());

        // Test symlink
        let symlink_node = root
            .node_ops
            .symlink("my_link", "/proc/self/fd/0")
            .expect("symlink my_link");
        assert_eq!(
            symlink_node.node_ops.readlink().expect("readlink my_link"),
            "/proc/self/fd/0"
        );

        let lookup_link = root.node_ops.lookup("my_link").expect("lookup my_link");
        assert_eq!(
            lookup_link.node_ops.readlink().expect("readlink lookup_link"),
            "/proc/self/fd/0"
        );

        // Test readdir
        let file_ops = root.node_ops.open(OpenFlags::READ).expect("open devfs dir");
        let readdir_bytes = file_ops.readdir().expect("readdir");
        let readdir_str = core::str::from_utf8(&readdir_bytes).expect("utf8 readdir");
        assert!(readdir_str.contains("shm\n"));
        assert!(readdir_str.contains("my_link\n"));
        assert!(readdir_str.contains("null\n"));
        assert!(readdir_str.contains("fd\n"));

        // Test unlink
        root.node_ops.unlink("my_link").expect("unlink my_link");
        assert!(root.node_ops.lookup("my_link").is_err());

        // Test rmdir
        root.node_ops.rmdir("shm").expect("rmdir shm");
        assert!(root.node_ops.lookup("shm").is_err());
    }
}
