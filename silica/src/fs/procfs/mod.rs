// SPDX-License-Identifier: GPL-2.0

//! Process Filesystem (`/proc`).
//!
//! Exposes kernel process state, system statistics, and hardware metadata
//! as a virtual in-memory filesystem.

pub mod pid;
pub mod self_;
pub mod sys;
pub mod system;
pub mod template;

use alloc::{boxed::Box, format, sync::Arc, vec::Vec};

use crate::{
    api::errno::Result,
    fs::vfs::{FileOps, FileSystem, INode, INodeAttr, Mode, NodeOps, OpenFlags, StatFs},
    proc::{allproc_find, allproc_iter, Pid},
};

pub use pid::ProcPidDirOps;
pub use self_::ProcSelfSymlinkOps;
pub use sys::ProcSysDirOps;
pub use system::*;
pub use template::{
    CallbackProcFileOps, DynamicProcFileNodeOps, DynamicProcFileOps, ProcDir, ProcDirFileOps,
    ProcDirNodeOps, ProcDirOps, ProcFile, ProcFileNodeOps, ProcFileOps, ProcSym, ProcSymNodeOps,
    ProcSymOps, StaticDirEntry,
};

/// Declarative static directory table for root `/proc` entries.
pub static STATIC_PROC_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "self",
        ino: 100,
        is_dir: false,
        constructor: || {
            Arc::new(INode::new_with_attr(
                Box::new(ProcSymNodeOps::new(ProcSelfSymlinkOps)),
                100,
                INodeAttr {
                    size: 1,
                    mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))
        },
    },
    StaticDirEntry {
        name: "mounts",
        ino: 101,
        is_dir: false,
        constructor: || {
            Arc::new(INode::new_with_attr(
                Box::new(ProcFileNodeOps::new_ro(CallbackProcFileOps::new(generate_mounts))),
                101,
                INodeAttr {
                    size: 0,
                    mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))
        },
    },
    StaticDirEntry {
        name: "meminfo",
        ino: 102,
        is_dir: false,
        constructor: || {
            Arc::new(INode::new_with_attr(
                Box::new(ProcFileNodeOps::new_ro(CallbackProcFileOps::new(generate_meminfo))),
                102,
                INodeAttr {
                    size: 0,
                    mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))
        },
    },
    StaticDirEntry {
        name: "cpuinfo",
        ino: 103,
        is_dir: false,
        constructor: || {
            Arc::new(INode::new_with_attr(
                Box::new(ProcFileNodeOps::new_ro(CallbackProcFileOps::new(generate_cpuinfo))),
                103,
                INodeAttr {
                    size: 0,
                    mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))
        },
    },
    StaticDirEntry {
        name: "version",
        ino: 104,
        is_dir: false,
        constructor: || {
            Arc::new(INode::new_with_attr(
                Box::new(ProcFileNodeOps::new_ro(CallbackProcFileOps::new(generate_version))),
                104,
                INodeAttr {
                    size: 0,
                    mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))
        },
    },
    StaticDirEntry {
        name: "stat",
        ino: 105,
        is_dir: false,
        constructor: || {
            Arc::new(INode::new_with_attr(
                Box::new(ProcFileNodeOps::new_ro(CallbackProcFileOps::new(generate_stat))),
                105,
                INodeAttr {
                    size: 0,
                    mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))
        },
    },
    StaticDirEntry {
        name: "uptime",
        ino: 106,
        is_dir: false,
        constructor: || {
            Arc::new(INode::new_with_attr(
                Box::new(ProcFileNodeOps::new_ro(CallbackProcFileOps::new(generate_uptime))),
                106,
                INodeAttr {
                    size: 0,
                    mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))
        },
    },
    StaticDirEntry {
        name: "cmdline",
        ino: 107,
        is_dir: false,
        constructor: || {
            Arc::new(INode::new_with_attr(
                Box::new(ProcFileNodeOps::new_ro(CallbackProcFileOps::new(generate_cmdline))),
                107,
                INodeAttr {
                    size: 0,
                    mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                },
            ))
        },
    },
    StaticDirEntry {
        name: "sys",
        ino: 108,
        is_dir: true,
        constructor: || {
            Arc::new(INode::new(
                Box::new(ProcSysDirOps),
                Mode::DIR | Mode::RUSR | Mode::XUSR | Mode::RGRP | Mode::XGRP | Mode::ROTH | Mode::XOTH,
                108,
            ))
        },
    },
];

/// `/proc` root directory node operations.
pub struct ProcRootDirOps;

impl NodeOps for ProcRootDirOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        for entry in STATIC_PROC_ENTRIES {
            if entry.name == name {
                return Ok((entry.constructor)());
            }
        }

        if let Some(pid) = name
            .parse::<u32>()
            .ok()
            .and_then(Pid::from_u32)
            .filter(|&pid| allproc_find(pid).is_some())
        {
            return Ok(Arc::new(INode::new(
                Box::new(ProcPidDirOps::new(pid)),
                Mode::DIR
                    | Mode::RUSR
                    | Mode::XUSR
                    | Mode::RGRP
                    | Mode::XGRP
                    | Mode::ROTH
                    | Mode::XOTH,
                (pid.as_u32() as usize) << 16,
            )));
        }

        crate::return_errno!(ENOENT, "unknown entry in /proc")
    }

    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EISDIR, "cannot write to directory");
        }
        Ok(Box::new(ProcRootDirFileOps))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::DIR | Mode::RUSR | Mode::XUSR | Mode::RGRP | Mode::XGRP | Mode::ROTH | Mode::XOTH,
            nlink: 2,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

pub struct ProcRootDirFileOps;

impl FileOps for ProcRootDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(b".\n..\n");
        for entry in STATIC_PROC_ENTRIES {
            out.extend_from_slice(format!("{}\n", entry.name).as_bytes());
        }
        for proc in allproc_iter() {
            out.extend_from_slice(format!("{}\n", proc.pid.as_u32()).as_bytes());
        }
        Ok(out)
    }
}

/// The ProcFS filesystem representation.
pub struct ProcFs {
    root: Arc<INode>,
}

impl ProcFs {
    pub fn new() -> Self {
        Self {
            root: Arc::new(INode::new(
                Box::new(ProcRootDirOps),
                Mode::DIR | Mode::RUSR | Mode::XUSR | Mode::RGRP | Mode::XGRP | Mode::ROTH | Mode::XOTH,
                1,
            )),
        }
    }
}

impl Default for ProcFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for ProcFs {
    fn name(&self) -> &'static str {
        "proc"
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

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;
    use crate::fs::vfs::OpenFlags;

    #[ktest]
    fn procfs_system_files() {
        let procfs = ProcFs::new();
        let root = procfs.root().expect("procfs root");

        // 1. /proc/version
        let version_node = root.node_ops.lookup("version").expect("lookup version");
        let version_ops = version_node.node_ops.open(OpenFlags::READ).expect("open version");
        let mut buf = [0u8; 128];
        let n = version_ops.read(0, &mut buf).expect("read version");
        let s = core::str::from_utf8(&buf[..n]).expect("utf8 version");
        assert!(s.starts_with("Linux version 6.6.0-silica"));

        // 2. /proc/cmdline
        let cmdline_node = root.node_ops.lookup("cmdline").expect("lookup cmdline");
        let cmdline_ops = cmdline_node.node_ops.open(OpenFlags::READ).expect("open cmdline");
        let mut buf = [0u8; 128];
        let n = cmdline_ops.read(0, &mut buf).expect("read cmdline");
        let s = core::str::from_utf8(&buf[..n]).expect("utf8 cmdline");
        assert!(s.contains("init=/bin/sh"));

        // 3. /proc/meminfo
        let meminfo_node = root.node_ops.lookup("meminfo").expect("lookup meminfo");
        let meminfo_ops = meminfo_node.node_ops.open(OpenFlags::READ).expect("open meminfo");
        let mut buf = [0u8; 256];
        let n = meminfo_ops.read(0, &mut buf).expect("read meminfo");
        let s = core::str::from_utf8(&buf[..n]).expect("utf8 meminfo");
        assert!(s.contains("MemTotal:"));
        assert!(s.contains("MemFree:"));

        // 4. /proc/mounts
        let mounts_node = root.node_ops.lookup("mounts").expect("lookup mounts");
        let mounts_ops = mounts_node.node_ops.open(OpenFlags::READ).expect("open mounts");
        let mut buf = [0u8; 256];
        let n = mounts_ops.read(0, &mut buf).expect("read mounts");
        let s = core::str::from_utf8(&buf[..n]).expect("utf8 mounts");
        assert!(s.contains("rootfs"));

        // 5. /proc/sys/kernel/osrelease
        let sys_node = root.node_ops.lookup("sys").expect("lookup sys");
        let kernel_node = sys_node.node_ops.lookup("kernel").expect("lookup kernel");
        let osrelease_node = kernel_node.node_ops.lookup("osrelease").expect("lookup osrelease");
        let osrelease_ops = osrelease_node.node_ops.open(OpenFlags::READ).expect("open osrelease");
        let mut buf = [0u8; 32];
        let n = osrelease_ops.read(0, &mut buf).expect("read osrelease");
        assert_eq!(&buf[..n], b"6.6.0-silica\n");
    }

    #[ktest]
    fn procfs_self_symlink() {
        let procfs = ProcFs::new();
        let root = procfs.root().expect("procfs root");

        let self_node = root.node_ops.lookup("self").expect("lookup self");
        assert!(self_node.is_symlink());
        let target = self_node.node_ops.readlink().expect("readlink self");
        assert!(!target.is_empty());
    }
}
