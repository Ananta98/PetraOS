// SPDX-License-Identifier: GPL-2.0

//! Filesystem subsystem for PetraOS.

pub mod devfs;
pub mod epoll;
pub mod exfat;
pub mod ext4;
pub mod fd;
pub mod initramfs;
pub mod pipe;
pub mod poll;
pub mod procfs;
pub mod registry;
pub mod socket;
pub mod sysfs;
pub mod template;
pub mod tmpfs;
pub mod vfs;

pub use fd::*;
pub use initramfs::read_file_from_initramfs;
pub use procfs::ProcFs;
pub use registry::*;
pub use sysfs::SysFs;
pub use tmpfs::TmpFs;
pub use vfs::{
    AccessFlags, File, FileOps, FileSystem, INode, LookupFlags, Mode, OpenFlags, PathNode,
    SeekAnchor, check_permission, dcache::DEntry, lookup, mount::Mount, open, open_at, perms, root,
    set_root, sync_all,
};

use alloc::{boxed::Box, string::String, sync::Arc};

use crate::fs::initramfs::{InitramfsDirNodeOps, InitramfsFs};

/// Brings up the virtual filesystem, following the Linux boot order:
///
/// 1. `init_mount_tree()` — the initramfs rootfs mounted at `/`;
/// 2. mount-point directories (`/dev`, `/proc`, `/sys`, `/mnt`) populated
///    under the root, in the order user space expects;
/// 3. the pseudo-filesystems the shell and tools rely on mounted: procfs
///    and sysfs (`mount -t proc`, `mount -t sysfs` in Linux initramfs);
/// 4. a writable, scratch `/tmp` directory.
///
/// Block-backed filesystems are deliberately *not* probed here — mounting
/// regular data partitions is a user-space decision issued via `mount(2)`
/// (see `sys_mount` and `registry`), like the Linux `mount(8)` flow.
pub fn init() {
    init_mount_tree();
    mount_core_filesystems();
    prepare_scratch_directories();
    ostd::info!("VFS initialized: mounted Initramfs at /, /proc, /sys, /mnt, and /tmp ready");
}

// ---------------------------------------------------------------------------
// Mount traversal helpers
// ---------------------------------------------------------------------------

/// A resolved mount point: the parent directory node on the underlying
/// filesystem plus the mount-point dentry itself.
struct MountPoint {
    parent: PathNode,
    dentry: Arc<DEntry>,
}

/// Creates (or looks up) a plain directory directly under the rootfs,
/// returning the root node and the directory's dentry. Used both for
/// mount-point staging and for scratch directories.
fn root_directory(dir: &str) -> Option<(PathNode, Arc<DEntry>)> {
    let Ok(root_node) = root() else {
        ostd::warn!("VFS: no root fs yet, delaying /{}", dir);
        return None;
    };

    if let Some(child) = root_node.dentry.lookup_child(dir) {
        return Some((root_node, child));
    }

    let dir_inode = Arc::new(INode::new(
        Box::new(InitramfsDirNodeOps::new()),
        mount_dir_mode(dir),
        root_mount_dir_ino(dir),
    ));
    let dentry = Arc::new(DEntry::new(
        String::from(dir),
        Some(dir_inode),
        Some(Arc::downgrade(&root_node.dentry)),
    ));
    root_node.dentry.add_child(dentry.clone());
    Some((root_node, dentry))
}

/// Creates (or looks up) a rootfs directory that can host a mount.
///
/// The returned mount point pairs the parent directory node on the
/// underlying filesystem with the "hole" dentry that the mounted
/// filesystem will cover.
fn prepare_mount_point(dir: &str) -> Option<MountPoint> {
    let Some((parent, dentry)) = root_directory(dir) else {
        ostd::warn!("VFS: mount point /{} unavailable", dir);
        return None;
    };
    Some(MountPoint { parent, dentry })
}

/// Mounts a filesystem instance at the given mount-point directory and
/// records the mount in the table (the analogue of `new_do_mount_kernfs`).
fn mount_fs(
    fs_instance: Arc<dyn FileSystem>,
    mount_dir: &str,
    source: &str,
    fstype: &str,
    opts: &str,
) {
    let Some(point) = prepare_mount_point(mount_dir) else {
        ostd::warn!("VFS: mount point /{} unavailable", mount_dir);
        return;
    };

    let Ok(fs_root) = fs_instance.root() else {
        ostd::error!("VFS: {} has no root inode", fstype);
        return;
    };

    let mount_dentry = Arc::new(DEntry::new(
        String::from(mount_dir),
        Some(fs_root),
        Some(Arc::downgrade(&point.dentry)),
    ));
    let new_mount = Arc::new(Mount::new(Arc::clone(&mount_dentry), fs_instance));

    let dir_node = PathNode {
        mount: point.parent.mount.clone(),
        dentry: Arc::clone(&point.dentry),
    };
    if let Err(e) = dir_node.mount(new_mount) {
        ostd::warn!("VFS: mounting /{} over rootfs failed: {:?}", mount_dir, e);
    }
    vfs::mount::register_mount(source, mount_dir, fstype, opts);
}

/// Creates a plain (non-mount) directory under the rootfs.
///
/// Analogous to `initramfs` `mkdir -p`: `/dev`, `/mnt`, and `/tmp` are kept
/// as ordinary rootfs directories rather than mounts.
fn create_root_directory(dir: &str) {
    root_directory(dir);
}

/// Reserved inode number of a root mount-point directory.
fn root_mount_dir_ino(dir: &str) -> usize {
    ROOT_MOUNT_DIR_INO
        .iter()
        .find(|(name, _)| *name == dir)
        .map(|(_, ino)| *ino)
        .unwrap_or(993)
}

// ---------------------------------------------------------------------------
// Boot staging, one function per Linux init stage
// ---------------------------------------------------------------------------

/// Mounts the initramfs rootfs as `/` — the analogue of Linux
/// `init_mount_tree()`.
fn init_mount_tree() {
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
}

/// Pseudo-filesystem and scratch mount points, mounted from the initramfs
/// root. Inode numbers are reserved in rootfs so the paths stay stable.
static ROOT_MOUNT_DIR_INO: [(&str, usize); 5] = [
    ("dev", 998),
    ("proc", 996),
    ("sys", 995),
    ("tmp", 994),
    ("mnt", 999),
];

/// Reserved permissions of the root mount-point directories. `/tmp` is
/// world-writable and sticky as mandated by POSIX.
fn mount_dir_mode(dir: &str) -> Mode {
    let dir_mode = Mode::DIR | Mode::RUSR | Mode::WUSR | Mode::XUSR | Mode::RGRP | Mode::ROTH;
    if dir == "tmp" {
        dir_mode | Mode::WGRP | Mode::XGRP | Mode::WOTH | Mode::XOTH | Mode::STICKY
    } else {
        dir_mode
    }
}

/// Assembles `/dev`, `/proc`, and `/sys`, mirroring the initramfs sequence
/// `mount -t devtmpfs`, `mount -t proc`, `mount -t sysfs`. `/dev` stays a
/// plain rootfs directory: devices expose through devfs on demand.
fn mount_core_filesystems() {
    create_root_directory("dev");

    mount_fs(
        Arc::new(ProcFs::new()),
        "proc",
        "proc",
        "proc",
        "rw,nosuid,nodev,noexec,relatime",
    );

    sysfs::init();
    mount_fs(
        Arc::new(SysFs::new()),
        "sys",
        "sysfs",
        "sysfs",
        "rw,nosuid,nodev,noexec,relatime",
    );
}

/// Prepares `/mnt` (future user-space mount points) and the writable
/// `/tmp` scratch directory POSIX tools rely on.
fn prepare_scratch_directories() {
    create_root_directory("mnt");
    create_root_directory("tmp");
}
