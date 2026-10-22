// SPDX-License-Identifier: GPL-2.0

//! Device Filesystem (`/dev`).
//!
//! Provides a dynamic, extensible character and block device node registry
//! for PetraOS drivers to register and unregister device nodes at runtime.

pub mod blk;
pub mod chr;
pub mod drm;
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
use alloc::string::String;
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use blk::GenericBlkNodeOps;
use chr::GenericChrNodeOps;
use null::NullNodeOps;
use random::{RandomNodeOps, UrandomNodeOps};
use zero::ZeroNodeOps;

/// Operations on the `/dev` root directory inode.
pub struct DevFsDirOps;

impl NodeOps for DevFsDirOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        // 1. Check static dev nodes first
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
            "fd" => {
                return Ok(Arc::new(INode::new_with_attr(
                    Box::new(DevSymlinkNodeOps {
                        target: "/proc/self/fd",
                    }),
                    6,
                    INodeAttr {
                        size: "/proc/self/fd".len(),
                        mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                        nlink: 1,
                        uid: 0,
                        gid: 0,
                        rdev: 0,
                    },
                )));
            }
            "stdin" => {
                return Ok(Arc::new(INode::new_with_attr(
                    Box::new(DevSymlinkNodeOps {
                        target: "/proc/self/fd/0",
                    }),
                    7,
                    INodeAttr {
                        size: "/proc/self/fd/0".len(),
                        mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                        nlink: 1,
                        uid: 0,
                        gid: 0,
                        rdev: 0,
                    },
                )));
            }
            "stdout" => {
                return Ok(Arc::new(INode::new_with_attr(
                    Box::new(DevSymlinkNodeOps {
                        target: "/proc/self/fd/1",
                    }),
                    8,
                    INodeAttr {
                        size: "/proc/self/fd/1".len(),
                        mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                        nlink: 1,
                        uid: 0,
                        gid: 0,
                        rdev: 0,
                    },
                )));
            }
            "stderr" => {
                return Ok(Arc::new(INode::new_with_attr(
                    Box::new(DevSymlinkNodeOps {
                        target: "/proc/self/fd/2",
                    }),
                    9,
                    INodeAttr {
                        size: "/proc/self/fd/2".len(),
                        mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                        nlink: 1,
                        uid: 0,
                        gid: 0,
                        rdev: 0,
                    },
                )));
            }
            _ => {}
        }

        // 2. Check character devices
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

        // 3. Check block devices
        for dev in crate::drivers::get_blkdevs() {
            if dev.name() == name {
                return Ok(Arc::new(INode::new(
                    Box::new(GenericBlkNodeOps::new(Arc::clone(&dev))),
                    Mode::BLOCK | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP,
                    100, // In real implementations, you'd use a unique inode number
                )));
            }
        }

        // 4. Check network devices
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
        Ok(Box::new(DevFsDirFileOps))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::DIR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::XUSR
                | Mode::RGRP
                | Mode::XGRP
                | Mode::ROTH
                | Mode::XOTH,
            nlink: 2,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

/// Directory file operations for reading `/dev`.
pub struct DevFsDirFileOps;

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
        out.extend_from_slice(b"dri\n");
        out.extend_from_slice(b"fd\n");
        out.extend_from_slice(b"stdin\n");
        out.extend_from_slice(b"stdout\n");
        out.extend_from_slice(b"stderr\n");

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

/// Symbolic link node operations in `/dev`.
pub struct DevSymlinkNodeOps {
    pub target: &'static str,
}

impl NodeOps for DevSymlinkNodeOps {
    fn readlink(&self) -> Result<String> {
        Ok(String::from(self.target))
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
pub struct DevFs;

impl DevFs {
    pub const fn new() -> Self {
        Self
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
        Ok(Arc::new(INode::new(
            Box::new(DevFsDirOps),
            Mode::DIR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::XUSR
                | Mode::RGRP
                | Mode::XGRP
                | Mode::ROTH
                | Mode::XOTH,
            0,
        )))
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
