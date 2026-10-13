// SPDX-License-Identifier: GPL-2.0

//! Filesystem subsystem for PetraOS.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;

pub mod devfs;
pub mod exfat;
pub mod ext2;
pub mod fd;
pub mod initramfs;
pub mod pipe;
pub mod poll;
pub mod procfs;
pub mod registry;
pub mod socket;
pub mod sysfs;
pub mod tmpfs;
pub mod vfs;

pub use fd::*;
pub use initramfs::read_file_from_initramfs;
pub use procfs::ProcFs;
pub use registry::*;
pub use sysfs::SysFs;
pub use tmpfs::TmpFs;
pub use vfs::{
    File, FileOps, FileSystem, INode, Mode, OpenFlags, PathNode, dcache::DEntry, lookup,
    mount::Mount, open, open_at, perms, root, set_root,
};

use crate::fs::initramfs::{InitramfsDirNodeOps, InitramfsFs};

/// Initializes the VFS root filesystem and mounts core device nodes and pseudo-filesystems.
pub fn init() {
    // 1. Initialize root filesystem from initramfs
    let initramfs_fs = Arc::new(InitramfsFs::new());
    let root_inode = initramfs_fs
        .root()
        .expect("failed to get initramfs root inode");
    let root_dentry = Arc::new(DEntry::new(String::new(), Some(root_inode), None));
    let root_mount = Arc::new(Mount::new(Arc::clone(&root_dentry), initramfs_fs));
    let root_node = PathNode {
        mount: root_mount,
        dentry: root_dentry,
    };
    vfs::set_root(root_node.clone());
    vfs::mount::register_mount("rootfs", "/", "rootfs", "rw,relatime");

    // 2. Ensure /dev mount directory exists in root
    if root_node.dentry.lookup_child("dev").is_none() {
        let dev_node_ops = Box::new(InitramfsDirNodeOps::new());
        let dev_inode = Arc::new(INode::new(
            dev_node_ops,
            Mode::DIR | Mode::RUSR | Mode::WUSR | Mode::XUSR | Mode::RGRP | Mode::ROTH,
            998,
        ));
        let dev_dentry = Arc::new(DEntry::new(
            "dev".into(),
            Some(dev_inode),
            Some(Arc::downgrade(&root_node.dentry)),
        ));
        root_node.dentry.add_child(dev_dentry);
    }

    // 3. Create /mnt mount directory if not present
    if root_node.dentry.lookup_child("mnt").is_none() {
        let mnt_node_ops = Box::new(InitramfsDirNodeOps::new());
        let mnt_inode = Arc::new(INode::new(
            mnt_node_ops,
            Mode::DIR | Mode::RUSR | Mode::WUSR | Mode::XUSR | Mode::RGRP | Mode::ROTH,
            999,
        ));
        let mnt_dentry = Arc::new(DEntry::new(
            "mnt".into(),
            Some(mnt_inode),
            Some(Arc::downgrade(&root_node.dentry)),
        ));
        root_node.dentry.add_child(mnt_dentry);
    }

    // 4. Create /proc mount directory and mount ProcFs
    let proc_dentry = if let Some(child) = root_node.dentry.lookup_child("proc") {
        child
    } else {
        let proc_node_ops = Box::new(InitramfsDirNodeOps::new());
        let proc_inode = Arc::new(INode::new(
            proc_node_ops,
            Mode::DIR | Mode::RUSR | Mode::WUSR | Mode::XUSR | Mode::RGRP | Mode::ROTH,
            996,
        ));
        let dentry = Arc::new(DEntry::new(
            "proc".into(),
            Some(proc_inode),
            Some(Arc::downgrade(&root_node.dentry)),
        ));
        root_node.dentry.add_child(dentry.clone());
        dentry
    };
    let proc_node = PathNode {
        mount: root_node.mount.clone(),
        dentry: proc_dentry.clone(),
    };
    let procfs = Arc::new(ProcFs::new());
    if let Ok(root_inode) = procfs.root() {
        let mount_dentry = Arc::new(DEntry::new(
            "proc".into(),
            Some(root_inode),
            Some(Arc::downgrade(&proc_dentry)),
        ));
        let proc_mount = Arc::new(Mount::new(mount_dentry, procfs));
        let _ = proc_node.mount(proc_mount);
        vfs::mount::register_mount("proc", "/proc", "proc", "rw,nosuid,nodev,noexec,relatime");
    }

    // 5. Create /sys mount directory and mount SysFs
    let sys_dentry = if let Some(child) = root_node.dentry.lookup_child("sys") {
        child
    } else {
        let sys_node_ops = Box::new(InitramfsDirNodeOps::new());
        let sys_inode = Arc::new(INode::new(
            sys_node_ops,
            Mode::DIR | Mode::RUSR | Mode::WUSR | Mode::XUSR | Mode::RGRP | Mode::ROTH,
            995,
        ));
        let dentry = Arc::new(DEntry::new(
            "sys".into(),
            Some(sys_inode),
            Some(Arc::downgrade(&root_node.dentry)),
        ));
        root_node.dentry.add_child(dentry.clone());
        dentry
    };
    let sys_node = PathNode {
        mount: root_node.mount.clone(),
        dentry: sys_dentry.clone(),
    };
    let sysfs = Arc::new(SysFs::new());
    if let Ok(root_inode) = sysfs.root() {
        let mount_dentry = Arc::new(DEntry::new(
            "sys".into(),
            Some(root_inode),
            Some(Arc::downgrade(&sys_dentry)),
        ));
        let sys_mount = Arc::new(Mount::new(mount_dentry, sysfs));
        let _ = sys_node.mount(sys_mount);
        vfs::mount::register_mount("sysfs", "/sys", "sysfs", "rw,nosuid,nodev,noexec,relatime");
    }

    // 6. Create /tmp directory if not present
    if root_node.dentry.lookup_child("tmp").is_none() {
        let tmp_node_ops = Box::new(InitramfsDirNodeOps::new());
        let tmp_inode = Arc::new(INode::new(
            tmp_node_ops,
            Mode::DIR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::XUSR
                | Mode::RGRP
                | Mode::WGRP
                | Mode::XGRP
                | Mode::ROTH
                | Mode::WOTH
                | Mode::XOTH
                | Mode::STICKY,
            994,
        ));
        let tmp_dentry = Arc::new(DEntry::new(
            "tmp".into(),
            Some(tmp_inode),
            Some(Arc::downgrade(&root_node.dentry)),
        ));
        root_node.dentry.add_child(tmp_dentry);
    }

    auto_probe_and_mount();

    ostd::info!("VFS initialized: mounted Initramfs at /, /dev, /proc, /sys, /mnt, and /tmp ready");
}

