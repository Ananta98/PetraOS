// SPDX-License-Identifier: GPL-2.0

//! The SysFS filesystem object.
//!
//! Requires the kernel object tree ([`super::kobject`]); registering
//! content happens at boot through [`super::init`], while mounting merely
//! wraps the tree root in VFS node operations. Mounting does not mutate the
//! tree, mirroring the Asterinas pattern of boot-time registration versus
//! user-controlled mounting.
use alloc::{boxed::Box, sync::Arc};

use crate::{
    uapi::errno::Result,
    fs::{
        sysfs::{
            inode::SysFsDirNodeOps,
            kobject::{self, sys_tree},
        },
        template::dir_mode,
        vfs::{FileSystem, INode, INodeAttr, StatFs},
    },
};

/// The SysFS pseudo-filesystem.
pub struct SysFs {
    root: Arc<INode>,
}

impl SysFs {
    /// Wraps the initialized kernel object tree in a filesystem instance.
    pub fn new() -> Self {
        // Always ensure the tree exists, even if `init` has not run yet;
        // the `Once` in `sys_tree` makes repeated construction safe (the
        // boot-time registration in `super::init` simply re-seeds the
        // static attributes).
        let tree = sys_tree();
        Self {
            root: Arc::new(INode::new_with_attr(
                Box::new(SysFsDirNodeOps::new(
                    Arc::clone(tree.root_dir()) as Arc<dyn kobject::SysNode>
                )),
                tree.root_dir().ino(),
                INodeAttr {
                    mode: dir_mode(),
                    nlink: 2,
                    ..Default::default()
                },
            )),
        }
    }
}

impl Default for SysFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for SysFs {
    fn name(&self) -> &'static str {
        "sysfs"
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
