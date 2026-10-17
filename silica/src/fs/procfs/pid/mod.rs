// SPDX-License-Identifier: GPL-2.0

//! Per-process (`/proc/<pid>/`) dynamic hierarchy.

pub mod cmdline;
pub mod cwd;
pub mod exe;
pub mod fd;
pub mod stat;
pub mod status;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};

use crate::{
    api::errno::{Errno, Result},
    fs::{
        procfs::template::{CallbackProcFileOps, ProcFileNodeOps, ProcSymNodeOps, ProcSymOps},
        vfs::{FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags},
    },
    proc::{allproc_find, Pid},
};

pub use cmdline::generate_proc_cmdline;
pub use cwd::ProcPidCwdSymlinkOps;
pub use exe::ProcPidExeSymlinkOps;
pub use fd::{ProcFdSymlinkOps, ProcPidFdDirFileOps, ProcPidFdDirOps};
pub use stat::generate_proc_stat;
pub use status::generate_proc_status;

// Tags for deterministic per-process inode addressing: ((pid << 16) | tag)
pub const TAG_DIR: u16 = 0x0000;
pub const TAG_EXE: u16 = 0x0001;
pub const TAG_CWD: u16 = 0x0002;
pub const TAG_MOUNTS: u16 = 0x0003;
pub const TAG_CMDLINE: u16 = 0x0004;
pub const TAG_STATUS: u16 = 0x0005;
pub const TAG_STAT: u16 = 0x0006;
pub const TAG_FD_DIR: u16 = 0x0010;

#[inline]
pub fn pid_ino(pid: Pid, tag: u16) -> usize {
    ((pid.as_u32() as usize) << 16) | (tag as usize)
}

pub struct ProcPidMountsSymlinkOps;

impl ProcSymOps for ProcPidMountsSymlinkOps {
    fn readlink(&self) -> Result<String> {
        Ok(String::from("/proc/mounts"))
    }
}

/// `/proc/<pid>` directory node operations.
pub struct ProcPidDirOps {
    pid: Pid,
}

impl ProcPidDirOps {
    pub fn new(pid: Pid) -> Self {
        Self { pid }
    }
}

impl NodeOps for ProcPidDirOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        let _proc = allproc_find(self.pid).ok_or(Errno::ENOENT)?;

        match name {
            "exe" => Ok(Arc::new(INode::new_with_attr(
                Box::new(ProcSymNodeOps::new(ProcPidExeSymlinkOps { pid: self.pid })),
                pid_ino(self.pid, TAG_EXE),
                INodeAttr {
                    size: 16,
                    mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))),
            "cwd" => Ok(Arc::new(INode::new_with_attr(
                Box::new(ProcSymNodeOps::new(ProcPidCwdSymlinkOps)),
                pid_ino(self.pid, TAG_CWD),
                INodeAttr {
                    size: 1,
                    mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))),
            "mounts" => Ok(Arc::new(INode::new_with_attr(
                Box::new(ProcSymNodeOps::new(ProcPidMountsSymlinkOps)),
                pid_ino(self.pid, TAG_MOUNTS),
                INodeAttr {
                    size: 12,
                    mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))),
            "cmdline" => {
                let pid = self.pid;
                Ok(Arc::new(INode::new_with_attr(
                    Box::new(ProcFileNodeOps::new_ro(CallbackProcFileOps::new(move || {
                        generate_proc_cmdline(pid)
                    }))),
                    pid_ino(self.pid, TAG_CMDLINE),
                    INodeAttr {
                        size: 0,
                        mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                        nlink: 1,
                        uid: 0,
                        gid: 0,
                        rdev: 0,
                    },
                )))
            }
            "status" => {
                let pid = self.pid;
                Ok(Arc::new(INode::new_with_attr(
                    Box::new(ProcFileNodeOps::new_ro(CallbackProcFileOps::new(move || {
                        if let Some(p) = allproc_find(pid) {
                            generate_proc_status(&p)
                        } else {
                            Vec::new()
                        }
                    }))),
                    pid_ino(self.pid, TAG_STATUS),
                    INodeAttr {
                        size: 0,
                        mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                        nlink: 1,
                        uid: 0,
                        gid: 0,
                        rdev: 0,
                    },
                )))
            }
            "stat" => {
                let pid = self.pid;
                Ok(Arc::new(INode::new_with_attr(
                    Box::new(ProcFileNodeOps::new_ro(CallbackProcFileOps::new(move || {
                        if let Some(p) = allproc_find(pid) {
                            generate_proc_stat(&p)
                        } else {
                            Vec::new()
                        }
                    }))),
                    pid_ino(self.pid, TAG_STAT),
                    INodeAttr {
                        size: 0,
                        mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                        nlink: 1,
                        uid: 0,
                        gid: 0,
                        rdev: 0,
                    },
                )))
            }
            "fd" => Ok(Arc::new(INode::new(
                Box::new(ProcPidFdDirOps { pid: self.pid }),
                Mode::DIR
                    | Mode::RUSR
                    | Mode::XUSR
                    | Mode::RGRP
                    | Mode::XGRP
                    | Mode::ROTH
                    | Mode::XOTH,
                pid_ino(self.pid, TAG_FD_DIR),
            ))),
            _ => crate::return_errno!(ENOENT, "unknown entry in /proc/[pid]"),
        }
    }

    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EISDIR, "cannot write to directory");
        }
        Ok(Box::new(ProcPidDirFileOps))
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

pub struct ProcPidDirFileOps;

impl FileOps for ProcPidDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        Ok(b".\n..\nexe\ncwd\ncmdline\nstatus\nstat\nmounts\nfd\n".to_vec())
    }
}
