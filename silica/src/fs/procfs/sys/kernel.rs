// SPDX-License-Identifier: GPL-2.0

//! `/proc/sys/kernel` parameters.

use alloc::{boxed::Box, format, string::String, sync::Arc, vec::Vec};
use spin::RwLock;

use crate::{
    api::errno::{Errno, Result},
    fs::{
        procfs::template::DynamicProcFileNodeOps,
        vfs::{FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags, SeekAnchor},
    },
};

static HOSTNAME: RwLock<Option<String>> = RwLock::new(None);

/// Retrieves the current system hostname.
pub fn get_hostname() -> String {
    HOSTNAME
        .read()
        .clone()
        .unwrap_or_else(|| String::from("silica"))
}

/// Sets the system hostname.
pub fn set_hostname(name: String) {
    *HOSTNAME.write() = Some(name);
}

/// `/proc/sys/kernel` directory node operations.
pub struct ProcSysKernelDirOps;

impl NodeOps for ProcSysKernelDirOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        match name {
            "hostname" => Ok(Arc::new(INode::new_with_attr(
                Box::new(ProcHostnameNodeOps),
                2010,
                INodeAttr {
                    size: 0,
                    mode: Mode::FILE | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))),
            "osrelease" => Ok(Arc::new(INode::new_with_attr(
                Box::new(DynamicProcFileNodeOps::new(|| b"6.6.0-silica\n".to_vec())),
                2011,
                INodeAttr {
                    size: 13,
                    mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))),
            "ostype" => Ok(Arc::new(INode::new_with_attr(
                Box::new(DynamicProcFileNodeOps::new(|| b"Linux\n".to_vec())),
                2012,
                INodeAttr {
                    size: 6,
                    mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))),
            "version" => Ok(Arc::new(INode::new_with_attr(
                Box::new(DynamicProcFileNodeOps::new(|| {
                    b"#1 PREEMPT Mon Oct 12 2026\n".to_vec()
                })),
                2013,
                INodeAttr {
                    size: 28,
                    mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))),
            "pid_max" => Ok(Arc::new(INode::new_with_attr(
                Box::new(DynamicProcFileNodeOps::new(|| b"32768\n".to_vec())),
                2014,
                INodeAttr {
                    size: 6,
                    mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))),
            _ => crate::return_errno!(ENOENT, "unknown entry in /proc/sys/kernel"),
        }
    }

    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EISDIR, "cannot write to directory");
        }
        Ok(Box::new(ProcSysKernelDirFileOps))
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

pub struct ProcSysKernelDirFileOps;

impl FileOps for ProcSysKernelDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        Ok(b".\n..\nhostname\nosrelease\nostype\nversion\npid_max\n".to_vec())
    }
}

pub struct ProcHostnameNodeOps;

impl NodeOps for ProcHostnameNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        let name = format!("{}\n", get_hostname());
        Ok(Box::new(ProcHostnameFileOps {
            data: name.into_bytes(),
        }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::FILE | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

pub struct ProcHostnameFileOps {
    data: Vec<u8>,
}

impl FileOps for ProcHostnameFileOps {
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
        if let Ok(s) = core::str::from_utf8(buf) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                set_hostname(String::from(trimmed));
            }
        }
        Ok(buf.len())
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
