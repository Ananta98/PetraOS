// SPDX-License-Identifier: GPL-2.0

//! Core FileSystem trait for the VFS.

use super::inode::INode;
use crate::uapi::errno::Result;
use alloc::sync::Arc;

/// Filesystem-wide statistics, as reported by `statfs(2)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatFs {
    /// Filesystem block size.
    pub bsize: u64,
    /// Total data blocks.
    pub blocks: u64,
    /// Free blocks.
    pub bfree: u64,
    /// Free blocks available to unprivileged users.
    pub bavail: u64,
    /// Total inodes.
    pub files: u64,
    /// Free inodes.
    pub ffree: u64,
    /// Maximum filename length.
    pub namelen: u64,
}

/// The common trait implemented by every mounted filesystem driver in PetraOS.
pub trait FileSystem: Send + Sync {
    /// Returns the human-readable name or driver type (e.g. "initramfs", "devfs", "ext2").
    fn name(&self) -> &'static str;

    /// Retrieves the root INode of this filesystem.
    fn root(&self) -> Result<Arc<INode>>;

    /// Reports filesystem-wide statistics.
    fn statfs(&self) -> Result<StatFs> {
        crate::return_errno!(ENOSYS, "statfs not implemented");
    }

    /// Flushes all dirty filesystem state to storage. Called before unmount.
    fn sync(&self) -> Result<()> {
        Ok(())
    }
}
