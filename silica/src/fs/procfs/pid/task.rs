// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/task` — per-thread views of a process.
//!
//! `task/<tid>` mirrors `/proc/<pid>` for each live thread: identical entry
//! set built by the shared [`super::shared_pid_entry`] builder, except `stat`
//! renders thread-specific fields ([`super::stat::TaskStatFileOps`]) and
//! `task` itself is not nested further (matching Linux).

use alloc::{format, string::String, sync::Arc, vec::Vec};

use crate::{
    api::errno::{Errno, Result},
    fs::procfs::template::{ProcDirOps, dir_node},
    fs::vfs::INode,
    proc::{Pid, Tid, allproc_find},
};

/// `/proc/<pid>/task` directory node operations.
pub struct ProcPidTaskDirOps {
    pub pid: Pid,
}

impl ProcPidTaskDirOps {
    /// Creates a `task` handler for `pid`.
    pub const fn new(pid: Pid) -> Self {
        Self { pid }
    }
}

impl ProcDirOps for ProcPidTaskDirOps {
    fn lookup_child(&self, name: &str) -> Result<Arc<INode>> {
        let tid_num = name.parse::<u32>().map_err(|_| Errno::ENOENT)?;
        let tid = Tid::from_u32(tid_num).ok_or(Errno::ENOENT)?;
        let proc = allproc_find(self.pid).ok_or(Errno::ENOENT)?;

        // Threads list is authoritative; the leading task id is always its
        // own entry so `/proc/<pid>/task/<pid>` keeps working like Linux.
        let exists = {
            let inner = proc.inner.lock();
            inner.threads.iter().any(|t| t.tid == tid) || self.pid.as_u32() == tid_num
        };
        if !exists {
            crate::return_errno!(ENOENT, "thread not found");
        }

        Ok(dir_node(
            ProcTaskThreadDirOps::new(self.pid, tid_num),
            task_thread_ino(self.pid, tid_num),
        ))
    }

    fn list_entries(&self) -> Vec<String> {
        let Some(proc) = allproc_find(self.pid) else {
            return Vec::new();
        };
        let inner = proc.inner.lock();
        if inner.threads.is_empty() {
            alloc::vec![format!("{}", self.pid.as_u32())]
        } else {
            inner
                .threads
                .iter()
                .map(|t| format!("{}", t.tid.as_u32()))
                .collect()
        }
    }
}

/// `/proc/<pid>/task/<tid>` directory node operations: the thread view.
pub struct ProcTaskThreadDirOps {
    pub pid: Pid,
    pub tid: u32,
}

impl ProcTaskThreadDirOps {
    /// Creates a thread directory handler.
    pub const fn new(pid: Pid, tid: u32) -> Self {
        Self { pid, tid }
    }
}

impl ProcDirOps for ProcTaskThreadDirOps {
    fn lookup_child(&self, name: &str) -> Result<Arc<INode>> {
        // Same common entries as the process view ...
        if let Some(node) = super::shared_pid_entry(self.pid, name) {
            return Ok(node);
        }

        // ... with the stat file overridden to describe *this thread*.
        if name == "stat" {
            return Ok(crate::fs::procfs::template::file_node(
                super::stat::TaskStatFileOps {
                    pid: self.pid,
                    tid: self.tid,
                },
                super::pid_ino(self.pid, super::TAG_STAT),
            ));
        }

        crate::return_errno!(ENOENT, "unknown entry in /proc/[pid]/task/[tid]");
    }

    fn list_entries(&self) -> Vec<String> {
        // Thread views drop `task` (no nesting); everything else comes
        // straight from the shared entry set.
        super::SHARED_ENTRIES
            .iter()
            .map(|s| String::from(*s))
            .collect()
    }
}

/// Inode number of a `task/<tid>` directory: pid and tid layered above a
/// fixed tag disjoint from the per-process tags.
fn task_thread_ino(pid: Pid, tid: u32) -> usize {
    ((pid.as_u32() as usize) << 32) | ((tid as usize) << 16) | 0x8000
}
