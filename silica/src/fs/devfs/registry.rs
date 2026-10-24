// SPDX-License-Identifier: GPL-2.0

//! Device Filesystem Inode & Device Registry.
//!
//! Provides a centralized, dynamic registry for device nodes (character devices,
//! block devices, directories, and symlinks) in `/dev`.

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::String,
    sync::Arc,
    vec::Vec,
};
use spin::RwLock;

use crate::fs::vfs::{INode, INodeAttr, Mode, NodeOps};

/// Entry in the DevFS registry.
pub enum DevNodeEntry {
    /// Inode operations with specified mode and device number.
    Node {
        ops_factory: Box<dyn Fn() -> Box<dyn NodeOps> + Send + Sync>,
        mode: Mode,
        rdev: u64,
    },
    /// A directory node.
    Directory {
        ops_factory: Box<dyn Fn() -> Box<dyn NodeOps> + Send + Sync>,
        mode: Mode,
    },
    /// A symbolic link.
    Symlink {
        target: String,
    },
}

pub struct DevFsRegistry {
    entries: RwLock<BTreeMap<String, DevNodeEntry>>,
}

impl DevFsRegistry {
    pub const fn new() -> Self {
        Self {
            entries: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn register_node<F>(&self, name: &str, mode: Mode, rdev: u64, ops_factory: F)
    where
        F: Fn() -> Box<dyn NodeOps> + Send + Sync + 'static,
    {
        self.entries.write().insert(
            String::from(name),
            DevNodeEntry::Node {
                ops_factory: Box::new(ops_factory),
                mode,
                rdev,
            },
        );
    }

    pub fn register_dir<F>(&self, name: &str, mode: Mode, ops_factory: F)
    where
        F: Fn() -> Box<dyn NodeOps> + Send + Sync + 'static,
    {
        self.entries.write().insert(
            String::from(name),
            DevNodeEntry::Directory {
                ops_factory: Box::new(ops_factory),
                mode,
            },
        );
    }

    pub fn register_symlink(&self, name: &str, target: &str) {
        self.entries.write().insert(
            String::from(name),
            DevNodeEntry::Symlink {
                target: String::from(target),
            },
        );
    }

    pub fn lookup(&self, name: &str, ino: usize) -> Option<Arc<INode>> {
        let entries = self.entries.read();
        let entry = entries.get(name)?;
        match entry {
            DevNodeEntry::Node { ops_factory, mode, rdev } => {
                let attr = INodeAttr {
                    size: 0,
                    mode: *mode,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: *rdev,
                };
                Some(Arc::new(INode::new_with_attr(ops_factory(), ino, attr)))
            }
            DevNodeEntry::Directory { ops_factory, mode } => {
                let attr = INodeAttr {
                    size: 0,
                    mode: *mode | Mode::DIR,
                    nlink: 2,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                };
                Some(Arc::new(INode::new_with_attr(ops_factory(), ino, attr)))
            }
            DevNodeEntry::Symlink { target } => {
                let attr = INodeAttr {
                    size: target.len(),
                    mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                };
                Some(Arc::new(INode::new_with_attr(
                    Box::new(super::DevSymlinkNodeOps { target: target.clone() }),
                    ino,
                    attr,
                )))
            }
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.entries.read().keys().cloned().collect()
    }
}

/// Global device filesystem registry.
pub static GLOBAL_DEVFS_REGISTRY: DevFsRegistry = DevFsRegistry::new();

/// Registers a custom device node into `/dev`.
pub fn register_dev_node<F>(name: &str, mode: Mode, rdev: u64, ops_factory: F)
where
    F: Fn() -> Box<dyn NodeOps> + Send + Sync + 'static,
{
    GLOBAL_DEVFS_REGISTRY.register_node(name, mode, rdev, ops_factory);
}

/// Registers a directory node into `/dev`.
pub fn register_dev_dir<F>(name: &str, mode: Mode, ops_factory: F)
where
    F: Fn() -> Box<dyn NodeOps> + Send + Sync + 'static,
{
    GLOBAL_DEVFS_REGISTRY.register_dir(name, mode, ops_factory);
}

/// Registers a symlink into `/dev`.
pub fn register_dev_symlink(name: &str, target: &str) {
    GLOBAL_DEVFS_REGISTRY.register_symlink(name, target);
}
