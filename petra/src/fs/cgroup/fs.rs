// SPDX-License-Identifier: GPL-2.0

//! The cgroup v2 (cgroup2) filesystem.
//!
//! Exposes the unified cgroup v2 hierarchy conforming to the Linux specification.
//! Typically mounted at `/sys/fs/cgroup`.

use alloc::sync::Arc;

use crate::{
    fs::{
        cgroup::{
            node::cgroup_dir_to_inode,
            tree::default_cgroup_tree,
        },
        vfs::{FileSystem, INode, StatFs},
    },
    uapi::errno::Result,
};

/// Linux cgroup2 magic number (0x63677270).
pub const CGROUP2_SUPER_MAGIC: u64 = 0x63677270;

/// The cgroup v2 FileSystem implementation.
pub struct Cgroup2Fs {
    root: Arc<INode>,
}

impl Cgroup2Fs {
    /// Creates a new cgroup v2 filesystem wrapping the global root cgroup.
    pub fn new() -> Self {
        let tree = default_cgroup_tree();
        Self {
            root: cgroup_dir_to_inode(Arc::clone(&tree.root)),
        }
    }
}

impl Default for Cgroup2Fs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for Cgroup2Fs {
    fn name(&self) -> &'static str {
        "cgroup2"
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
