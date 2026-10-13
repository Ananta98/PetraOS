// SPDX-License-Identifier: GPL-2.0

//! `/proc/sys` kernel parameters hierarchy.

pub mod kernel;

use alloc::{boxed::Box, sync::Arc, vec::Vec};

use crate::{
    api::errno::Result,
    fs::vfs::{FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags},
};

pub use kernel::{get_hostname, set_hostname, ProcSysKernelDirOps};

/// `/proc/sys` directory node operations.
pub struct ProcSysDirOps;

impl NodeOps for ProcSysDirOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        match name {
            "kernel" => Ok(Arc::new(INode::new(
                Box::new(ProcSysKernelDirOps),
                Mode::DIR
                    | Mode::RUSR
                    | Mode::XUSR
                    | Mode::RGRP
                    | Mode::XGRP
                    | Mode::ROTH
                    | Mode::XOTH,
                2001,
            ))),
            _ => crate::return_errno!(ENOENT, "unknown entry in /proc/sys"),
        }
    }

    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EISDIR, "cannot write to directory");
        }
        Ok(Box::new(ProcSysDirFileOps))
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

pub struct ProcSysDirFileOps;

impl FileOps for ProcSysDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        Ok(b".\n..\nkernel\n".to_vec())
    }
}
