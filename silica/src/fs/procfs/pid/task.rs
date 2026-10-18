// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/task` directory and thread subdirectories.

use alloc::{boxed::Box, format, sync::Arc, vec::Vec};

use crate::{
    api::errno::{Errno, Result},
    fs::{
        procfs::{
            pid::{
                cmdline::generate_proc_cmdline,
                comm::generate_proc_comm,
                cwd::ProcPidCwdSymlinkOps,
                exe::ProcPidExeSymlinkOps,
                fd::ProcPidFdDirOps,
                pid_ino,
                stat::generate_proc_stat_for_task,
                statm::generate_proc_statm,
                status::generate_proc_status,
                ProcPidMountsSymlinkOps, TAG_CMDLINE, TAG_COMM, TAG_CWD, TAG_EXE, TAG_FD_DIR,
                TAG_MOUNTS, TAG_STAT, TAG_STATM, TAG_STATUS,
            },
            template::{CallbackProcFileOps, ProcFileNodeOps, ProcSymNodeOps},
        },
        vfs::{FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags},
    },
    proc::{allproc_find, Pid, Tid},
};

/// `/proc/<pid>/task` directory node operations.
pub struct ProcPidTaskDirOps {
    pub pid: Pid,
}

impl NodeOps for ProcPidTaskDirOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        let tid_num = name.parse::<u32>().map_err(|_| Errno::ENOENT)?;
        let tid = Tid::from_u32(tid_num).ok_or(Errno::ENOENT)?;
        let proc = allproc_find(self.pid).ok_or(Errno::ENOENT)?;

        let exists = {
            let inner = proc.inner.lock();
            inner.threads.iter().any(|t| t.tid == tid) || self.pid.as_u32() == tid_num
        };
        if !exists {
            crate::return_errno!(ENOENT, "thread not found");
        }

        Ok(Arc::new(INode::new(
            Box::new(ProcTaskThreadDirOps {
                pid: self.pid,
                tid: tid_num,
            }),
            Mode::DIR
                | Mode::RUSR
                | Mode::XUSR
                | Mode::RGRP
                | Mode::XGRP
                | Mode::ROTH
                | Mode::XOTH,
            ((self.pid.as_u32() as usize) << 32) | ((tid_num as usize) << 16) | 0x8000,
        )))
    }

    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EISDIR, "cannot write to directory");
        }
        Ok(Box::new(ProcPidTaskDirFileOps { pid: self.pid }))
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

pub struct ProcPidTaskDirFileOps {
    pid: Pid,
}

impl FileOps for ProcPidTaskDirFileOps {
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
            let inner = proc.inner.lock();
            if inner.threads.is_empty() {
                out.extend_from_slice(format!("{}\n", self.pid.as_u32()).as_bytes());
            } else {
                for t in &inner.threads {
                    out.extend_from_slice(format!("{}\n", t.tid.as_u32()).as_bytes());
                }
            }
        }
        Ok(out)
    }
}

/// `/proc/<pid>/task/<tid>` directory node operations.
pub struct ProcTaskThreadDirOps {
    pub pid: Pid,
    pub tid: u32,
}

impl NodeOps for ProcTaskThreadDirOps {
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
                let tid = self.tid;
                Ok(Arc::new(INode::new_with_attr(
                    Box::new(ProcFileNodeOps::new_ro(CallbackProcFileOps::new(move || {
                        if let Some(p) = allproc_find(pid) {
                            generate_proc_stat_for_task(&p, Some(tid))
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
            "statm" => {
                let pid = self.pid;
                Ok(Arc::new(INode::new_with_attr(
                    Box::new(ProcFileNodeOps::new_ro(CallbackProcFileOps::new(move || {
                        if let Some(p) = allproc_find(pid) {
                            generate_proc_statm(&p)
                        } else {
                            Vec::new()
                        }
                    }))),
                    pid_ino(self.pid, TAG_STATM),
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
            "comm" => {
                let pid = self.pid;
                Ok(Arc::new(INode::new_with_attr(
                    Box::new(ProcFileNodeOps::new_ro(CallbackProcFileOps::new(move || {
                        if let Some(p) = allproc_find(pid) {
                            generate_proc_comm(&p)
                        } else {
                            Vec::new()
                        }
                    }))),
                    pid_ino(self.pid, TAG_COMM),
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
            _ => crate::return_errno!(ENOENT, "unknown entry in /proc/[pid]/task/[tid]"),
        }
    }

    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EISDIR, "cannot write to directory");
        }
        Ok(Box::new(ProcTaskThreadDirFileOps))
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

pub struct ProcTaskThreadDirFileOps;

impl FileOps for ProcTaskThreadDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        Ok(b".\n..\nexe\ncwd\ncmdline\nstatus\nstat\nstatm\ncomm\nmounts\nfd\n".to_vec())
    }
}
