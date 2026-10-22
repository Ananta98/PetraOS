// SPDX-License-Identifier: GPL-2.0

//! Process Filesystem (`/proc`).
//!
//! A virtual filesystem exposing kernel process state, system statistics and
//! hardware metadata as synthetic files. Files here are ephemeral views: the
//! content is generated at each open from live kernel state through the
//! [`template`] module.
//!
//! Layout — mirroring both the Linux `fs/proc` tree and the Asterinas
//! procfs reference (`kernel/core/src/fs/fs_impls/procfs`):
//!
//! - root (this module): `/proc/*` static files,
//! - [`pid`]: per-process `/proc/<pid>/*` subdirectories,
//! - [`self_`]: `/proc/self` symlink to the caller,
//! - [`sys`]: kernel parameters (sysctl) hierarchy,
//! - one flat module per root file, matching upstream names
//!   (`meminfo.rs`, `loadavg.rs`, ...).

pub mod cmdline;
pub mod cpuinfo;
pub mod diskstats;
pub mod filesystems;
pub mod loadavg;
pub mod meminfo;
pub mod mounts;
pub mod partitions;
pub mod pid;
pub mod self_;
pub mod stat;
pub mod sys;
pub mod template;
pub mod uptime;
pub mod version;

use alloc::{format, string::String, sync::Arc, vec::Vec};

use crate::{
    fs::vfs::{FileSystem, INode, StatFs},
    proc::{Pid, allproc_find, allproc_iter},
    uapi::errno::Result,
};

/// The ProcFS filesystem representation.
pub struct ProcFs {
    root: Arc<INode>,
}

impl ProcFs {
    /// Creates a procfs instance with a fresh root directory.
    pub fn new() -> Self {
        Self {
            root: template::dir_node(ProcRootDirOps, PROC_ROOT_INO),
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

/// Inode number allocation for `/proc` (consumed by the root files, `sys`
/// and `pid` modules).
///
/// The root directory itself is inode 1. Static root entries claim 100..,
/// `/proc/sys` hierarchies 2000.., and per-process inodes are composed as
/// `((pid << 16) | tag)` (see `pid::pid_ino`). Keeping the ranges in one
/// documented block makes collisions visible at a glance.
///
/// Inode numbers are identifiers, not stable content addresses: they only
/// need to be unique within the mounted procfs instance.
const PROC_ROOT_INO: usize = 1;

/// Reserved root entry inode numbers (the 100.. block). Each root file
/// module consumes its own through `new_inode`.
pub(crate) const PROC_ROOT_INO_MOUNTS: usize = 101;
pub(crate) const PROC_ROOT_INO_MEMINFO: usize = 102;
pub(crate) const PROC_ROOT_INO_CPUINFO: usize = 103;
pub(crate) const PROC_ROOT_INO_VERSION: usize = 104;
pub(crate) const PROC_ROOT_INO_STAT: usize = 105;
pub(crate) const PROC_ROOT_INO_UPTIME: usize = 106;
pub(crate) const PROC_ROOT_INO_CMDLINE: usize = 107;
pub(crate) const PROC_ROOT_INO_LOADAVG: usize = 109;
pub(crate) const PROC_ROOT_INO_DISKSTATS: usize = 110;
pub(crate) const PROC_ROOT_INO_FILESYSTEMS: usize = 111;
pub(crate) const PROC_ROOT_INO_PARTITIONS: usize = 112;

/// Declarative static table of root `/proc` entries.
///
/// The same table drives both [`ProcRootDirOps::lookup_child`] and entry
/// enumeration, following the `("name", ctor)` tuple layout of the Asterinas
/// reference.
static PROC_ROOT_ENTRIES: &[template::StaticDirEntry] = &[
    template::StaticDirEntry {
        name: "self",
        ctor: self_::new_self_inode,
    },
    template::StaticDirEntry {
        name: "mounts",
        ctor: mounts::MountsFileOps::new_inode,
    },
    template::StaticDirEntry {
        name: "meminfo",
        ctor: meminfo::MemInfoFileOps::new_inode,
    },
    template::StaticDirEntry {
        name: "cpuinfo",
        ctor: cpuinfo::CpuInfoFileOps::new_inode,
    },
    template::StaticDirEntry {
        name: "version",
        ctor: version::VersionFileOps::new_inode,
    },
    template::StaticDirEntry {
        name: "stat",
        ctor: stat::StatFileOps::new_inode,
    },
    template::StaticDirEntry {
        name: "uptime",
        ctor: uptime::UptimeFileOps::new_inode,
    },
    template::StaticDirEntry {
        name: "cmdline",
        ctor: cmdline::CmdLineFileOps::new_inode,
    },
    template::StaticDirEntry {
        name: "sys",
        ctor: sys::new_sys_inode,
    },
    template::StaticDirEntry {
        name: "loadavg",
        ctor: loadavg::LoadAvgFileOps::new_inode,
    },
    template::StaticDirEntry {
        name: "diskstats",
        ctor: diskstats::DiskStatsFileOps::new_inode,
    },
    template::StaticDirEntry {
        name: "filesystems",
        ctor: filesystems::FileSystemsFileOps::new_inode,
    },
    template::StaticDirEntry {
        name: "partitions",
        ctor: partitions::PartitionsFileOps::new_inode,
    },
];

/// `/proc` root directory: static synthetic files plus one `<pid>` directory
/// per live process.
struct ProcRootDirOps;

impl template::ProcDirOps for ProcRootDirOps {
    fn lookup_child(&self, name: &str) -> Result<Arc<INode>> {
        // Static entries first: numeric names can never collide with them.
        if let Some(child) = template::lookup_static_entry(PROC_ROOT_ENTRIES, name) {
            return Ok(child);
        }

        // Then the dynamic `/proc/<pid>` directories of live processes.
        if let Some(pid) = name
            .parse::<u32>()
            .ok()
            .and_then(Pid::from_u32)
            .filter(|&pid| allproc_find(pid).is_some())
        {
            return Ok(template::dir_node(
                pid::ProcPidDirOps::new(pid),
                pid::pid_ino(pid, pid::TAG_DIR),
            ));
        }

        crate::return_errno!(ENOENT, "unknown entry in /proc");
    }

    fn list_entries(&self) -> Vec<String> {
        let mut entries = template::list_static_entries(PROC_ROOT_ENTRIES);
        entries.extend(
            allproc_iter()
                .into_iter()
                .map(|p| format!("{}", p.pid.as_u32())),
        );
        entries
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::fs::{procfs::ProcFs, vfs::OpenFlags};
    use ostd::prelude::ktest;

    fn root() -> Arc<INode> {
        ProcFs::new().root().expect("procfs root")
    }

    fn read_to_string(node: &Arc<INode>) -> String {
        let ops = node.node_ops.open(OpenFlags::READ).expect("open");
        let mut buf = [0u8; 256];
        let n = ops.read(0, &mut buf).expect("read");
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    fn lookup(root: &Arc<INode>, path: &[&str]) -> Arc<INode> {
        let mut cur = root.clone();
        for name in path {
            cur = cur.node_ops.lookup(name).expect("lookup");
        }
        cur
    }

    #[ktest]
    fn root_table_lookup_and_readdir() {
        let root_node = root();

        for entry in PROC_ROOT_ENTRIES {
            let name = entry.name;
            let child = root_node
                .node_ops
                .lookup(name)
                .unwrap_or_else(|_| panic!("lookup {name}"));
            assert!(child.node_ops.getattr().is_ok());
        }

        let file_ops = root_node.node_ops.open(OpenFlags::READ).expect("open root");
        let listing_buf = file_ops.readdir().expect("readdir root");
        let listing = String::from_utf8_lossy(&listing_buf);
        for entry in PROC_ROOT_ENTRIES {
            assert!(
                listing.split('\n').any(|l| l == entry.name),
                "readdir lists {name}",
                name = entry.name
            );
        }
    }

    #[ktest]
    fn version_file() {
        let node = lookup(&root(), &["version"]);
        assert!(read_to_string(&node).starts_with("Linux version 6.6.0-silica"));
    }

    #[ktest]
    fn cmdline_file() {
        let node = lookup(&root(), &["cmdline"]);
        assert!(read_to_string(&node).contains("init=/bin/sh"));
    }

    #[ktest]
    fn meminfo_file() {
        let node = lookup(&root(), &["meminfo"]);
        let content = read_to_string(&node);
        assert!(content.contains("MemTotal:"));
        assert!(content.contains("MemFree:"));
    }

    #[ktest]
    fn mounts_file() {
        let node = lookup(&root(), &["mounts"]);
        assert!(read_to_string(&node).contains("rootfs"));
    }

    #[ktest]
    fn sys_kernel_osrelease() {
        let node = lookup(&root(), &["sys", "kernel", "osrelease"]);
        let ops = node.node_ops.open(OpenFlags::READ).expect("open osrelease");
        let mut buf = [0u8; 32];
        let n = ops.read(0, &mut buf).expect("read osrelease");
        assert_eq!(&buf[..n], b"6.6.0-silica\n");
    }

    #[ktest]
    fn self_symlink() {
        let node = lookup(&root(), &["self"]);
        assert!(node.is_symlink());
        let target = node.node_ops.readlink().expect("readlink self");
        assert!(!target.is_empty());
    }

    #[ktest]
    fn root_entries_carry_file_modes() {
        // Spot-check that per-file ops structs really follow the shared
        // ProcFileOps trait rather than ad-hoc node implementations.
        let node = lookup(&root(), &["uptime"]);
        let attr = node.node_ops.getattr().expect("getattr uptime");
        assert!(attr.mode.contains(crate::fs::vfs::Mode::FILE));
    }
}
