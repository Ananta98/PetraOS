// SPDX-License-Identifier: GPL-2.0

//! SysFS Inode and File Operations Adapters.
//!
//! Projects `SysNode`, `SysDirNode`, `SysAttrNode`, and `SysSymlinkNode` into VFS `NodeOps` and `FileOps`.

use alloc::{boxed::Box, format, string::String, sync::Arc, vec::Vec};

use crate::{
    api::errno::{Errno, Result},
    fs::vfs::{FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags, SeekAnchor},
};

use super::kobject::SysNode;

/// Node operations for a SysFS directory.
pub struct SysFsDirNodeOps {
    node: Arc<dyn SysNode>,
}

impl SysFsDirNodeOps {
    pub fn new(node: Arc<dyn SysNode>) -> Self {
        Self { node }
    }
}

impl NodeOps for SysFsDirNodeOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        let child = self.node.get_child(name).ok_or(Errno::ENOENT)?;
        let ino = child.ino();
        if child.is_dir() {
            Ok(Arc::new(INode::new_with_attr(
                Box::new(SysFsDirNodeOps::new(child)),
                ino,
                INodeAttr {
                    size: 0,
                    mode: Mode::DIR
                        | Mode::RUSR
                        | Mode::XUSR
                        | Mode::RGRP
                        | Mode::XGRP
                        | Mode::ROTH
                        | Mode::XOTH,
                    nlink: 2,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            )))
        } else if child.is_symlink() {
            let target_len = child.readlink().map(|s| s.len()).unwrap_or(1);
            Ok(Arc::new(INode::new_with_attr(
                Box::new(SysFsSymlinkNodeOps::new(child)),
                ino,
                INodeAttr {
                    size: target_len,
                    mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            )))
        } else {
            let mode = if child.is_writable() {
                Mode::FILE | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH
            } else {
                Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH
            };
            Ok(Arc::new(INode::new_with_attr(
                Box::new(SysFsAttrNodeOps::new(child)),
                ino,
                INodeAttr {
                    size: 0,
                    mode,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            )))
        }
    }

    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EISDIR, "cannot write to directory");
        }
        Ok(Box::new(SysFsDirFileOps {
            node: Arc::clone(&self.node),
        }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::DIR
                | Mode::RUSR
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

/// Directory file operations iterating over SysFS children.
pub struct SysFsDirFileOps {
    node: Arc<dyn SysNode>,
}

impl FileOps for SysFsDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(b".\n..\n");
        for (name, _is_dir) in self.node.list_children() {
            out.extend_from_slice(format!("{}\n", name).as_bytes());
        }
        Ok(out)
    }
}

/// Node operations for an attribute file.
pub struct SysFsAttrNodeOps {
    node: Arc<dyn SysNode>,
}

impl SysFsAttrNodeOps {
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
        Ok(Box::new(SysFsAttrFileOps {
            data,
            node: Arc::clone(&self.node),
        }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        let mode = if self.node.is_writable() {
            Mode::FILE | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH
        } else {
            Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH
        };
        Ok(INodeAttr {
            size: 0,
            mode,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

/// File operations for reading/writing a SysFS attribute.
pub struct SysFsAttrFileOps {
    data: Vec<u8>,
    node: Arc<dyn SysNode>,
}

impl FileOps for SysFsAttrFileOps {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let offset = offset as usize;
        if offset >= self.data.len() {
            return Ok(0);
        }
        let available = &self.data[offset..];
        let copy_len = buf.len().min(available.len());
        buf[..copy_len].copy_from_slice(&available[..copy_len]);
        Ok(copy_len)
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        self.node.write_attr(buf)
    }

    fn seek(&self, curr_offset: u64, anchor: SeekAnchor) -> Result<u64> {
        let len = self.data.len() as i64;
        let new_offset = match anchor {
            SeekAnchor::Start(pos) => pos as i64,
            SeekAnchor::Current(delta) => curr_offset as i64 + delta,
            SeekAnchor::End(delta) => len + delta,
        };
        if new_offset < 0 {
            return Err(Errno::EINVAL);
        }
        Ok(new_offset as u64)
    }
}

/// Node operations for a SysFS symbolic link.
pub struct SysFsSymlinkNodeOps {
    node: Arc<dyn SysNode>,
}

impl SysFsSymlinkNodeOps {
    pub fn new(node: Arc<dyn SysNode>) -> Self {
        Self { node }
    }
}

impl NodeOps for SysFsSymlinkNodeOps {
    fn readlink(&self) -> Result<String> {
        self.node.readlink()
    }

    fn getattr(&self) -> Result<INodeAttr> {
        let target_len = self.node.readlink().map(|s| s.len()).unwrap_or(1);
        Ok(INodeAttr {
            size: target_len,
            mode: Mode::LINK | Mode::from_bits_truncate(0o777),
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}
