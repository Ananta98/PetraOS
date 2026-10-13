// SPDX-License-Identifier: GPL-2.0

//! Mount point and PathNode abstractions for the VFS.

use alloc::sync::Arc;
use spin::RwLock;

use crate::api::errno::{Errno, Result};
use super::dcache::DEntry;
use super::fs::FileSystem;

/// Represents a mounted filesystem instance.
pub struct Mount {
    /// The mount point in the parent filesystem (if any).
    pub mount_point: RwLock<Option<PathNode>>,
    /// The root dentry of this mounted filesystem.
    pub root: Arc<DEntry>,
    /// The filesystem implementation driving this mount.
    pub fs: Arc<dyn FileSystem>,
}

impl Mount {
    /// Creates a new Mount instance for a filesystem root.
    pub fn new(root: Arc<DEntry>, fs: Arc<dyn FileSystem>) -> Self {
        Self {
            mount_point: RwLock::new(None),
            root,
            fs,
        }
    }
}

/// Represents a specific location in the VFS, pairing a dentry with its mount instance.
#[derive(Clone)]
pub struct PathNode {
    /// The mount instance this node belongs to.
    pub mount: Arc<Mount>,
    /// The directory entry.
    pub dentry: Arc<DEntry>,
}

impl PathNode {
    /// Mounts another filesystem on top of this path node.
    pub fn mount(&self, new_mount: Arc<Mount>) -> Result<()> {
        self.dentry.mounts.write().push(new_mount.clone());
        *new_mount.mount_point.write() = Some(self.clone());
        Ok(())
    }

    /// Unmounts the filesystem whose root this node is.
    ///
    /// Fails with `EINVAL` if the node is not the root of a non-root mount, and
    /// with `EBUSY` if another filesystem is still mounted somewhere beneath it.
    /// Open files are not tracked per mount yet, so those do not block unmount.
    pub fn umount(&self) -> Result<()> {
        if !Arc::ptr_eq(&self.dentry, &self.mount.root) {
            return Err(Errno::EINVAL);
        }
        let mount_point = self.mount.mount_point.read().clone().ok_or(Errno::EINVAL)?;

        if has_submounts(&self.mount.root) {
            return Err(Errno::EBUSY);
        }

        self.mount.fs.sync()?;

        mount_point
            .dentry
            .mounts
            .write()
            .retain(|m| !Arc::ptr_eq(m, &self.mount));
        *self.mount.mount_point.write() = None;
        // Drop cached entries of the departing filesystem.
        self.mount.root.children.write().clear();
        Ok(())
    }
}

/// Returns whether any cached dentry beneath `dentry` has a filesystem mounted on it.
fn has_submounts(dentry: &Arc<DEntry>) -> bool {
    if !dentry.mounts.read().is_empty() {
        return true;
    }
    dentry.children.read().values().any(has_submounts)
}

/// Global root mount node of the VFS namespace.
pub static VFS_ROOT: RwLock<Option<PathNode>> = RwLock::new(None);

/// Retrieves a clone of the global root PathNode.
pub fn root() -> Result<PathNode> {
    VFS_ROOT.read().clone().ok_or(Errno::ENOENT)
}

/// Sets the global root PathNode.
pub fn set_root(node: PathNode) {
    *VFS_ROOT.write() = Some(node);
}

/// Metadata record of an active filesystem mount.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountRecord {
    pub source: alloc::string::String,
    pub target: alloc::string::String,
    pub fstype: alloc::string::String,
    pub options: alloc::string::String,
}

/// Global registry of active mounts.
pub static ACTIVE_MOUNTS: RwLock<alloc::vec::Vec<MountRecord>> = RwLock::new(alloc::vec::Vec::new());

/// Registers an active mount.
pub fn register_mount(source: &str, target: &str, fstype: &str, options: &str) {
    let mut mounts = ACTIVE_MOUNTS.write();
    if let Some(existing) = mounts.iter_mut().find(|m| m.target == target) {
        existing.source = alloc::string::String::from(source);
        existing.fstype = alloc::string::String::from(fstype);
        existing.options = alloc::string::String::from(options);
    } else {
        mounts.push(MountRecord {
            source: alloc::string::String::from(source),
            target: alloc::string::String::from(target),
            fstype: alloc::string::String::from(fstype),
            options: alloc::string::String::from(options),
        });
    }
}

/// Unregisters an active mount at `target`.
pub fn unregister_mount(target: &str) {
    let mut mounts = ACTIVE_MOUNTS.write();
    if let Some(pos) = mounts.iter().rposition(|m| m.target == target) {
        mounts.remove(pos);
    }
}

/// Returns a clone of all registered active mounts.
pub fn get_mounts() -> alloc::vec::Vec<MountRecord> {
    ACTIVE_MOUNTS.read().clone()
}

