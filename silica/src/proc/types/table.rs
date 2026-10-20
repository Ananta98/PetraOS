// SPDX-License-Identifier: GPL-2.0

//! Global process table with primary PID and secondary PGID/SID indexing.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    vec::Vec,
};
use ostd::sync::{RwLock, SpinLock};

use super::{
    pid::{PID_INIT, Pid, PidAllocator},
    proc::{Proc, ProcState},
};

/// Central process registry maintaining primary and secondary group/session indices.
pub struct ProcessTable {
    pub allproc: BTreeMap<Pid, Arc<Proc>>,
    pub pgrp_map: BTreeMap<Pid, BTreeSet<Pid>>,
    pub session_map: BTreeMap<Pid, BTreeSet<Pid>>,
}

impl ProcessTable {
    pub const fn new() -> Self {
        Self {
            allproc: BTreeMap::new(),
            pgrp_map: BTreeMap::new(),
            session_map: BTreeMap::new(),
        }
    }

    /// Inserts a new process and indexes its PID, PGID, and SID.
    pub fn insert(&mut self, proc: Arc<Proc>) {
        let pid = proc.pid;
        let pgid = proc.pgid();
        let sid = proc.sid();

        self.pgrp_map.entry(pgid).or_default().insert(pid);
        self.session_map.entry(sid).or_default().insert(pid);
        self.allproc.insert(pid, proc);
    }

    /// Removes a process from primary and secondary indices.
    pub fn remove(&mut self, pid: Pid) -> Option<Arc<Proc>> {
        let proc = self.allproc.remove(&pid)?;
        let pgid = proc.pgid();
        let sid = proc.sid();

        if let Some(set) = self.pgrp_map.get_mut(&pgid) {
            set.remove(&pid);
            if set.is_empty() {
                self.pgrp_map.remove(&pgid);
            }
        }

        if let Some(set) = self.session_map.get_mut(&sid) {
            set.remove(&pid);
            if set.is_empty() {
                self.session_map.remove(&sid);
            }
        }

        Some(proc)
    }

    /// Updates the secondary process group index for `pid`.
    pub fn update_pgid(&mut self, pid: Pid, old_pgid: Pid, new_pgid: Pid) {
        if old_pgid == new_pgid {
            return;
        }
        if let Some(set) = self.pgrp_map.get_mut(&old_pgid) {
            set.remove(&pid);
            if set.is_empty() {
                self.pgrp_map.remove(&old_pgid);
            }
        }
        self.pgrp_map.entry(new_pgid).or_default().insert(pid);
    }

    /// Updates the secondary session index for `pid`.
    pub fn update_sid(&mut self, pid: Pid, old_sid: Pid, new_sid: Pid) {
        if old_sid == new_sid {
            return;
        }
        if let Some(set) = self.session_map.get_mut(&old_sid) {
            set.remove(&pid);
            if set.is_empty() {
                self.session_map.remove(&old_sid);
            }
        }
        self.session_map.entry(new_sid).or_default().insert(pid);
    }
}

/// Backwards compatibility alias for `ProcessTable`.
pub type ProcessTree = ProcessTable;

/// Global PID allocator.
pub static PID_ALLOCATOR: SpinLock<PidAllocator> = SpinLock::new(PidAllocator::new());

/// Global process table protected by RwLock.
pub static PROCTREE_LOCK: RwLock<ProcessTable> = RwLock::new(ProcessTable::new());

/// Inserts process into global table.
pub fn allproc_insert(proc: Arc<Proc>) {
    PROCTREE_LOCK.write().insert(proc);
}

/// Removes process from global table.
pub fn allproc_remove(pid: Pid) -> Option<Arc<Proc>> {
    PROCTREE_LOCK.write().remove(pid)
}

/// Finds process by PID.
pub fn allproc_find(pid: Pid) -> Option<Arc<Proc>> {
    PROCTREE_LOCK.read().allproc.get(&pid).cloned()
}

/// Zero-allocation visitor over all processes in the system.
pub fn allproc_for_each<F: FnMut(&Arc<Proc>)>(mut f: F) {
    let table = PROCTREE_LOCK.read();
    for proc in table.allproc.values() {
        f(proc);
    }
}

/// Zero-allocation visitor over processes belonging to a given PGID.
pub fn pgrp_for_each<F: FnMut(&Arc<Proc>)>(pgid: Pid, mut f: F) {
    let table = PROCTREE_LOCK.read();
    if let Some(members) = table.pgrp_map.get(&pgid) {
        for member_pid in members {
            if let Some(proc) = table.allproc.get(member_pid) {
                f(proc);
            }
        }
    }
}

/// Returns a list of processes belonging to a specific process group.
pub fn pgrp_members(pgid: Pid) -> Vec<Arc<Proc>> {
    let table = PROCTREE_LOCK.read();
    if let Some(members) = table.pgrp_map.get(&pgid) {
        members
            .iter()
            .filter_map(|pid| table.allproc.get(pid).cloned())
            .collect()
    } else {
        Vec::new()
    }
}

/// Total count of registered processes.
pub fn allproc_count() -> usize {
    PROCTREE_LOCK.read().allproc.len()
}

/// Heap-allocating snapshot of all processes (deprecated, prefer `allproc_for_each`).
pub fn allproc_iter() -> Vec<Arc<Proc>> {
    PROCTREE_LOCK.read().allproc.values().cloned().collect()
}

/// Traverses ancestors to find an active reaper or defaults to `init` (PID 1).
pub fn tree_find_reaper(mut ppid: Option<Pid>) -> Pid {
    let table = PROCTREE_LOCK.read();
    while let Some(parent_pid) = ppid {
        if let Some(parent) = table.allproc.get(&parent_pid) {
            let inner = parent.inner.lock();
            if inner.is_reaper && inner.state == ProcState::Alive {
                return parent_pid;
            }
            ppid = inner.ppid;
        } else {
            break;
        }
    }
    PID_INIT
}

/// Reparents orphaned children to a designated reaper.
pub fn tree_reparent_children(children: &[Pid], reaper_pid: Pid) {
    let table = PROCTREE_LOCK.read();
    for &child_pid in children {
        if let Some(child) = table.allproc.get(&child_pid) {
            child.inner.lock().ppid = Some(reaper_pid);
        }
    }
    if let Some(reaper) = table.allproc.get(&reaper_pid) {
        reaper.inner.lock().children.extend_from_slice(children);
    }
}
