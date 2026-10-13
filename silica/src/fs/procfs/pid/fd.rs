// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/fd/` directory and symlink operations.

use alloc::{boxed::Box, format, string::String, sync::Arc, vec::Vec};

use crate::{
    api::errno::{Errno, Result},
    fs::{
        procfs::template::{ProcSymNodeOps, ProcSymOps},
        vfs::{FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags},
    },
    proc::tree::{allproc_find, Pid},
};

use super::pid_ino;

pub const TAG_FD_BASE: u16 = 0x1000;

pub struct ProcPidFdDirOps {
    pub pid: Pid,
}

impl NodeOps for ProcPidFdDirOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        let fd: i32 = name.parse().map_err(|_| Errno::ENOENT)?;
        let proc = allproc_find(self.pid).ok_or(Errno::ENOENT)?;
        let _ = proc.fd_table.lock().get(fd)?;

        let fd_tag = TAG_FD_BASE | (fd as u16 & 0x0FFF);
        Ok(Arc::new(INode::new_with_attr(
            Box::new(ProcSymNodeOps::new(ProcFdSymlinkOps { pid: self.pid, fd })),
            pid_ino(self.pid, fd_tag),
            INodeAttr {
                size: 16,
                mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                nlink: 1,
                uid: 0,
                gid: 0,
                rdev: 0,
            },
        )))
    }

    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EISDIR, "cannot write to directory");
        }
        Ok(Box::new(ProcPidFdDirFileOps { pid: self.pid }))
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

pub struct ProcPidFdDirFileOps {
    pub pid: Pid,
}

impl FileOps for ProcPidFdDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(b".\n..\n");
        if let Some(proc) = allproc_find(self.pid) {
            let fd_table = proc.fd_table.lock();
            for (idx, slot) in fd_table.files.iter().enumerate() {
                if slot.is_some() {
                    out.extend_from_slice(format!("{}\n", idx).as_bytes());
                }
            }
        }
        Ok(out)
    }
}

pub struct ProcFdSymlinkOps {
    pub pid: Pid,
    pub fd: i32,
}

impl ProcSymOps for ProcFdSymlinkOps {
    fn readlink(&self) -> Result<String> {
        let proc = allproc_find(self.pid).ok_or(Errno::ENOENT)?;
        let file = proc.fd_table.lock().get(self.fd)?;

        if let Some(s) = file
            .abs_path
            .as_deref()
            .and_then(|p| core::str::from_utf8(p).ok())
        {
            return Ok(String::from(s));
        }

        // Fallbacks for pseudo-files and standard streams
        if file.inode.is_chr() {
            Ok(String::from("/dev/console"))
        } else if file.inode.is_fifo() {
            Ok(format!("pipe:[{}]", file.inode.ino()))
        } else if file.inode.is_socket() {
            Ok(format!("socket:[{}]", file.inode.ino()))
        } else {
            Ok(String::from("/dev/null"))
        }
    }
}
