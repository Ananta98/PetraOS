use crate::proc::pid_table::{PROCESS_TABLE, Pid};
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::sync::SpinLock;

// ---------------------------------------------------------------------------
// ProcessGroup
// ---------------------------------------------------------------------------

/// A Unix **process group** — a collection of processes that share a common
/// PGID and can be targeted together by job-control signals (e.g. `SIGTSTP`,
/// `SIGCONT`, `SIGHUP`).
///
/// # POSIX semantics
/// - Every process belongs to exactly one process group.
/// - A new process group is created by calling `setpgid(pid, pid)` or
///   `setsid()` on the process that will become the group leader.
/// - The process with `pid == pgid` is the **group leader**.
/// - When the leader exits, the group continues to exist until its last
///   member leaves.
///
/// # Design
/// `ProcessGroup` is reference-counted (`Arc`) and stored inside [`Process`]
/// so that processes can share the same group object without duplication.
/// The member list is guarded by a `SpinLock` for safe concurrent access
/// from multiple threads and CPUs.
#[derive(Debug)]
pub struct ProcessGroup {
    /// The process group ID — equal to the PID of the group leader.
    pub pgid: Pid,

    /// PIDs of all processes currently belonging to this group.
    ///
    /// Maintained in insertion order; duplicates are never added.
    members: SpinLock<Vec<Pid>>,
}

impl ProcessGroup {
    /// Create a new process group led by `leader_pid`.
    ///
    /// The leader is automatically added as the first member.
    pub fn new(leader_pid: Pid) -> Arc<Self> {
        Arc::new(Self {
            pgid: leader_pid,
            members: SpinLock::new(alloc::vec![leader_pid]),
        })
    }

    // -----------------------------------------------------------------------
    // Membership management
    // -----------------------------------------------------------------------

    /// Add `pid` to this process group if it is not already a member.
    pub fn add_member(&self, pid: Pid) {
        let mut members = self.members.lock();
        if !members.contains(&pid) {
            members.push(pid);
        }
    }

    /// Remove `pid` from this process group.
    ///
    /// Returns `true` if the process was a member and has been removed.
    pub fn remove_member(&self, pid: Pid) -> bool {
        let mut members = self.members.lock();
        if let Some(pos) = members.iter().position(|&p| p == pid) {
            members.swap_remove(pos);
            true
        } else {
            false
        }
    }

    /// Returns `true` if this process group currently has no members.
    pub fn is_empty(&self) -> bool {
        self.members.lock().is_empty()
    }

    /// Returns a snapshot of the current member PIDs.
    pub fn member_pids(&self) -> Vec<Pid> {
        self.members.lock().clone()
    }

    // -----------------------------------------------------------------------
    // Signal delivery
    // -----------------------------------------------------------------------

    /// Send `signal` to every member of this group that is still alive.
    ///
    /// Delivers the signal through each process's [`ProcessSignals`] handle.
    /// Members that have already exited (i.e. no longer in `PROCESS_TABLE`)
    /// are silently skipped.
    pub fn send_signal(&self, signal: u8) {
        let pids = self.member_pids();
        for pid in pids {
            if let Some(proc) = PROCESS_TABLE.get_process(pid) {
                proc.signals.queue.enqueue(crate::ipc::signal::types::SigInfo::kernel(signal as u32));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ProcessGroupTable
// ---------------------------------------------------------------------------

/// A kernel-wide registry mapping each PGID to its live [`ProcessGroup`].
///
/// Processes register their group here on creation / `setpgid`, and
/// deregister when a group becomes empty.  Storing groups in this table
/// enables `kill(-pgid, sig)` to locate the target group in O(1) without
/// scanning the entire process table.
pub struct ProcessGroupTable {
    table: SpinLock<alloc::collections::BTreeMap<Pid, Arc<ProcessGroup>>>,
}

impl ProcessGroupTable {
    /// Construct an empty table (suitable for use as a `static`).
    pub const fn new() -> Self {
        Self {
            table: SpinLock::new(alloc::collections::BTreeMap::new()),
        }
    }

    /// Register a newly-created process group.
    ///
    /// If a group with the same PGID already exists it is **replaced**;
    /// callers must ensure uniqueness (the PGID is derived from the leader
    /// PID which is guaranteed unique by `NEXT_PID`).
    pub fn register(&self, group: Arc<ProcessGroup>) {
        self.table.lock().insert(group.pgid, group);
    }

    /// Remove the process group with the given PGID from the registry.
    pub fn unregister(&self, pgid: Pid) {
        self.table.lock().remove(&pgid);
    }

    /// Look up a process group by PGID.
    ///
    /// Returns `None` if no group with that PGID is currently registered.
    pub fn get(&self, pgid: Pid) -> Option<Arc<ProcessGroup>> {
        self.table.lock().get(&pgid).cloned()
    }

    /// Returns `true` if a process group with `pgid` exists in the table.
    pub fn contains(&self, pgid: Pid) -> bool {
        self.table.lock().contains_key(&pgid)
    }
}

/// Kernel-wide process-group registry.
pub static PROCESS_GROUP_TABLE: ProcessGroupTable = ProcessGroupTable::new();
