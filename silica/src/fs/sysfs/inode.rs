// SPDX-License-Identifier: GPL-2.0

//! SysFS VFS projection layer.
//!
//! The kernel object tree ([`super::kobject`]) holds registration state;
//! the adapters in this file project it into VFS nodes for a mount:
//!
//! - [`SysFsDirNodeOps`] — directories, recursing into the tree on lookup,
//! - [`SysFsAttrNodeOps`] — attribute files (content snapshotted at open;
//!   writes pass through to the owning subsystem),
//! - [`SysFsSymlinkNodeOps`] — tree symlinks.
//!
//! Every node resolves its attribute set from the underlying `SysNode`, so
//! mode and inode numbers always agree with the tree state.

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};

use crate::{
    uapi::errno::{Errno, Result},
    fs::{
        template::{BufFileOps, WriteHook, dir_mode, ro_file_mode, rw_file_mode, symlink_mode},
        vfs::{FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags},
    },
};

use super::kobject::SysNode;

/// Node operations for a SysFS directory.
pub struct SysFsDirNodeOps {
    node: Arc<dyn SysNode>,
}

impl SysFsDirNodeOps {
    /// Wraps the tree node into directory node operations.
    pub fn new(node: Arc<dyn SysNode>) -> Self {
        Self { node }
    }
}

impl NodeOps for SysFsDirNodeOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        let child = self.node.get_child(name).ok_or(Errno::ENOENT)?;
        Ok(child_to_inode(&child))
    }

    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EISDIR, "cannot write to directory");
        }
        Ok(Box::new(SysFsDirFileOps {
            children: self.node.list_children(),
        }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            mode: dir_mode(),
            nlink: 2,
            ..Default::default()
        })
    }
}

/// Projects one tree node onto the matching VFS inode shape.
fn child_to_inode(child: &Arc<dyn SysNode>) -> Arc<INode> {
    let ino = child.ino();
    if child.is_dir() {
        Arc::new(INode::new_with_attr(
            Box::new(SysFsDirNodeOps::new(Arc::clone(child))),
            ino,
            INodeAttr {
                mode: dir_mode(),
                nlink: 2,
                ..Default::default()
            },
        ))
    } else if child.is_symlink() {
        let size = child.readlink().map(|s| s.len()).unwrap_or(0);
        Arc::new(INode::new_with_attr(
            Box::new(SysFsSymlinkNodeOps::new(Arc::clone(child))),
            ino,
            INodeAttr {
                size,
                mode: symlink_mode(),
                nlink: 1,
                ..Default::default()
            },
        ))
    } else {
        Arc::new(INode::new_with_attr(
            Box::new(SysFsAttrNodeOps::new(Arc::clone(child))),
            ino,
            INodeAttr {
                size: 0,
                mode: Mode::FILE | attr_mode(child.is_writable()),
                nlink: 1,
                ..Default::default()
            },
        ))
    }
}

/// Mode bits of an attribute file depending on writability.
fn attr_mode(writable: bool) -> Mode {
    if writable {
        rw_file_mode()
    } else {
        ro_file_mode()
    }
}

/// Directory file operations enumerating tree children.
pub struct SysFsDirFileOps {
    children: Vec<(String, bool)>,
}

impl FileOps for SysFsDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        let mut out = Vec::from(b".\n..\n");
        for (name, _is_dir) in &self.children {
            out.extend_from_slice(name.as_bytes());
            out.push(b'\n');
        }
        Ok(out)
    }
}

/// Node operations for an attribute file.
pub struct SysFsAttrNodeOps {
    node: Arc<dyn SysNode>,
}

impl SysFsAttrNodeOps {
    /// Wraps an attribute node into file node operations.
    pub fn new(node: Arc<dyn SysNode>) -> Self {
        Self { node }
    }
}

impl NodeOps for SysFsAttrNodeOps {
    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) && !self.node.is_writable() {
            crate::return_errno!(EPERM, "attribute is read-only");
        }
        let data = self.node.read_attr()?;
        let ops = if self.node.is_writable() {
            let node = Arc::clone(&self.node);
            let hook = Box::new(move |buf: &[u8]| node.write_attr(buf)) as WriteHook;
            BufFileOps::with_write_hook(data, hook)
        } else {
            BufFileOps::new(data)
        };
        Ok(Box::new(ops))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::FILE | attr_mode(self.node.is_writable()),
            nlink: 1,
            ..Default::default()
        })
    }
}

/// Node operations for a SysFS symbolic link.
pub struct SysFsSymlinkNodeOps {
    node: Arc<dyn SysNode>,
}

impl SysFsSymlinkNodeOps {
    /// Wraps a symlink node into symlink node operations.
    pub fn new(node: Arc<dyn SysNode>) -> Self {
        Self { node }
    }
}

impl NodeOps for SysFsSymlinkNodeOps {
    fn readlink(&self) -> Result<String> {
        self.node.readlink()
    }

    fn getattr(&self) -> Result<INodeAttr> {
        let size = self.node.readlink().map(|s| s.len()).unwrap_or(0);
        Ok(INodeAttr {
            size,
            mode: symlink_mode(),
            nlink: 1,
            ..Default::default()
        })
    }
}
