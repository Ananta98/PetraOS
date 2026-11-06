// SPDX-License-Identifier: GPL-2.0

//! Per-process `/proc/<pid>/` hierarchy.
//!
//! One directory per live process with a fixed set of entries; the same
//! layout is exposed a second time under `task/<tid>/` (see [`task`]), so a
//! shared builder ([`shared_pid_entry`]) constructs every common entry.
//! Enumeration follows the single `PID_ENTRIES` table so lookup and
//! `readdir` cannot disagree, mirroring the Asterinas entry-table pattern.

pub mod cmdline;
pub mod comm;
pub mod cwd;
pub mod cgroup;
pub mod environ;
pub mod exe;
pub mod fd;
pub mod io;
pub mod maps;
pub mod oom;
pub mod stat;
pub mod statm;
pub mod status;
pub mod task;

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    fs::procfs::template::{ProcDirOps, ProcFileOps, ProcSymOps, dir_node, file_node, rw_file_node, sym_node},
    fs::vfs::INode,
    proc::{Pid, allproc_find},
    uapi::errno::{Errno, Result},
};

pub use cwd::ProcPidCwdSymlinkOps;
pub use exe::ProcPidExeSymlinkOps;
pub use fd::{ProcFdSymlinkOps, ProcPidFdDirOps};
pub use task::ProcPidTaskDirOps;

/// Tags composing per-process inode numbers via [`pid_ino`].
///
/// Each entry needs a stable inode number that also communicates its role:
/// `(pid << 16) | tag` keeps directories, files and symlinks from colliding
/// while sharing the address space within one process.
pub const TAG_DIR: u16 = 0x0000;
pub const TAG_EXE: u16 = 0x0001;
pub const TAG_CWD: u16 = 0x0002;
pub const TAG_MOUNTS: u16 = 0x0003;
pub const TAG_CMDLINE: u16 = 0x0004;
pub const TAG_STATUS: u16 = 0x0005;
pub const TAG_STAT: u16 = 0x0006;
pub const TAG_STATM: u16 = 0x0007;
pub const TAG_COMM: u16 = 0x0008;
pub const TAG_TASK_DIR: u16 = 0x0009;
pub const TAG_IO: u16 = 0x0011;
pub const TAG_MAPS: u16 = 0x0012;
pub const TAG_ENVIRON: u16 = 0x0013;
pub const TAG_CGROUP: u16 = 0x0014;
pub const TAG_MOUNTINFO: u16 = 0x0015;
pub const TAG_OOM_SCORE_ADJ: u16 = 0x0016;
pub const TAG_OOM_SCORE: u16 = 0x0017;

/// Composes the per-entry inode number for `pid` from `tag`.
pub const fn pid_ino(pid: Pid, tag: u16) -> usize {
    ((pid.as_u32() as usize) << 16) | (tag as usize)
}

/// Entry names exposed under both `/proc/<pid>/` and `task/<tid>`.
///
/// The single source of truth for `readdir`; lookup handles exactly these
/// names plus the per-hierarchy extras (`task` under `/proc/<pid>/`).
const SHARED_ENTRIES: &[&str] = &[
    "exe", "cwd", "cmdline", "status", "stat", "statm", "comm", "mounts", "mountinfo", "oom_score_adj", "oom_score", "fd", "io", "maps",
    "environ", "cgroup",
];

/// Symlink `/proc/<pid>/mounts` -> `/proc/mounts`, matching Linux.
pub struct ProcPidMountsSymlinkOps;

impl ProcSymOps for ProcPidMountsSymlinkOps {
    fn readlink(&self) -> Result<String> {
        Ok(String::from("/proc/mounts"))
    }
}

pub struct ProcPidMountinfoFileOps;

impl ProcFileOps for ProcPidMountinfoFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let mounts = crate::fs::vfs::mount::get_mounts();
        let mut content = String::new();
        let mut id = 20;
        let is_chrooted = crate::proc::current_proc()
            .and_then(|p| p.root.lock().clone())
            .is_some();
        let has_sysroot = mounts.iter().any(|m| m.target == "/sysroot");

        for m in &mounts {
            let target = if is_chrooted && has_sysroot {
                if m.target == "/sysroot" {
                    "/"
                } else if let Some(sub) = m.target.strip_prefix("/sysroot") {
                    sub
                } else {
                    m.target.as_str()
                }
            } else {
                m.target.as_str()
            };
            let parent_id = if target == "/" { 1 } else { 20 };
            let _ = core::fmt::write(
                &mut content,
                format_args!(
                    "{} {} 0:{} / {} rw,relatime shared:1 - {} {} {}\n",
                    id, parent_id, id, target, m.fstype, m.source, m.options
                ),
            );
            id += 1;
        }
        if mounts.is_empty() {
            content.push_str("20 1 0:20 / / rw,relatime shared:1 - rootfs rootfs rw\n");
            content.push_str("21 20 0:21 / /dev rw,nosuid,relatime shared:2 - devfs devfs rw\n");
            content.push_str("22 20 0:22 / /proc rw,nosuid,nodev,noexec,relatime shared:3 - proc proc rw\n");
            content.push_str("23 20 0:23 / /sys rw,nosuid,nodev,noexec,relatime shared:4 - sysfs sysfs rw\n");
        }
        content.into_bytes()
    }
}

/// Builds the absolute `/proc/<pid>/<name>` node when shared with `task/`.
///
/// Returns `None` if the name is not part of the shared set (task
/// directories supply their own extras).
pub(super) fn shared_pid_entry(pid: Pid, name: &str) -> Option<Arc<INode>> {
    match name {
        "exe" => Some(sym_node(
            ProcPidExeSymlinkOps { pid },
            pid_ino(pid, TAG_EXE),
        )),
        "cwd" => Some(sym_node(ProcPidCwdSymlinkOps, pid_ino(pid, TAG_CWD))),
        "mounts" => Some(sym_node(ProcPidMountsSymlinkOps, pid_ino(pid, TAG_MOUNTS))),
        "cmdline" => Some(file_node(
            cmdline::PidCmdLineFileOps { pid },
            pid_ino(pid, TAG_CMDLINE),
        )),
        "status" => Some(file_node(
            status::PidStatusFileOps { pid },
            pid_ino(pid, TAG_STATUS),
        )),
        "stat" => Some(file_node(
            stat::PidStatFileOps { pid },
            pid_ino(pid, TAG_STAT),
        )),
        "statm" => Some(file_node(
            statm::PidStatmFileOps { pid },
            pid_ino(pid, TAG_STATM),
        )),
        "comm" => Some(file_node(
            comm::PidCommFileOps { pid },
            pid_ino(pid, TAG_COMM),
        )),
        "fd" => Some(dir_node(
            fd::ProcPidFdDirOps::new(pid),
            pid_ino(pid, fd::TAG_FD_DIR),
        )),
        "io" => Some(file_node(io::PidIoFileOps { pid }, pid_ino(pid, TAG_IO))),
        "maps" => Some(file_node(maps::PidMapsFileOps { pid }, pid_ino(pid, TAG_MAPS))),
        "environ" => Some(file_node(
            environ::PidEnvironFileOps { pid },
            pid_ino(pid, TAG_ENVIRON),
        )),
        "cgroup" => Some(file_node(
            cgroup::PidCgroupFileOps { pid },
            pid_ino(pid, TAG_CGROUP),
        )),
        "mountinfo" => Some(file_node(ProcPidMountinfoFileOps, pid_ino(pid, TAG_MOUNTINFO))),
        "oom_score_adj" => Some(rw_file_node(
            oom::PidOomScoreAdjFileOps { pid },
            pid_ino(pid, TAG_OOM_SCORE_ADJ),
        )),
        "oom_score" => Some(file_node(
            oom::PidOomScoreFileOps { pid },
            pid_ino(pid, TAG_OOM_SCORE),
        )),
        _ => None,
    }
}

/// `/proc/<pid>` directory node operations.
pub struct ProcPidDirOps {
    pid: Pid,
}

impl ProcPidDirOps {
    /// Creates a directory handler for `pid`.
    pub const fn new(pid: Pid) -> Self {
        Self { pid }
    }
}

impl ProcDirOps for ProcPidDirOps {
    fn lookup_child(&self, name: &str) -> Result<Arc<INode>> {
        // The process may exit between path resolution steps; treat missing
        // table entries as ENOENT, consistent with Linux.
        allproc_find(self.pid).ok_or(Errno::ENOENT)?;

        // The per-process-only subdirectory.
        if name == "task" {
            return Ok(dir_node(
                task::ProcPidTaskDirOps::new(self.pid),
                pid_ino(self.pid, TAG_TASK_DIR),
            ));
        }

        shared_pid_entry(self.pid, name).ok_or(Errno::ENOENT)
    }

    fn list_entries(&self) -> Vec<String> {
        SHARED_ENTRIES
            .iter()
            .chain(&["task"])
            .map(|s| String::from(*s))
            .collect()
    }
}
