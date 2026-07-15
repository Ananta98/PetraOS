use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};
use ostd::sync::SpinLock;

use crate::proc::thread::KernelThread;

// ---------------------------------------------------------------------------
// Tid
// ---------------------------------------------------------------------------

/// Thread ID counter.
///
/// Starts at 1; TID 0 is reserved as a sentinel "no thread" value, mirroring
/// the Linux convention for `task_struct::pid`.
static NEXT_TID: AtomicU32 = AtomicU32::new(1);

/// A unique thread identifier.
///
/// Analogous to `pid_t` in Linux when used as a thread ID (POSIX threads share
/// a PID but have individual TIDs).  The kernel represents each thread as an
/// independent schedulable unit and assigns it a unique `Tid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tid(u32);

impl Tid {
    /// Allocate the next available TID atomically.
    pub fn new() -> Self {
        Self(NEXT_TID.fetch_add(1, Ordering::Relaxed))
    }

    /// Construct a `Tid` from a known raw value.
    ///
    /// Use sparingly — primarily for well-known or sentinel TIDs.
    /// Does **not** advance `NEXT_TID`.
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

impl Default for Tid {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// ThreadTable
// ---------------------------------------------------------------------------

/// Global table mapping TIDs to live kernel threads.
///
/// Threads register themselves upon creation and deregister when they finish
/// execution, analogous to the Linux process table (`task_list`) but scoped to
/// kernel threads.
pub struct ThreadTable {
    table: SpinLock<BTreeMap<Tid, Arc<KernelThread>>>,
}

impl ThreadTable {
    /// Create an empty thread table.
    pub const fn new() -> Self {
        Self {
            table: SpinLock::new(BTreeMap::new()),
        }
    }

    /// Register a newly created thread.
    pub fn register(&self, thread: Arc<KernelThread>) {
        self.table.lock().insert(thread.tid(), thread);
    }

    /// Remove a finished thread from the table.
    pub fn unregister(&self, tid: Tid) {
        self.table.lock().remove(&tid);
    }

    /// Look up a thread by its TID, returning a clone of the `Arc` if found.
    pub fn get(&self, tid: Tid) -> Option<Arc<KernelThread>> {
        self.table.lock().get(&tid).cloned()
    }

    /// Iterate over all live threads that belong to the given process PID.
    ///
    /// Returns a `Vec` of cloned `Arc<KernelThread>` snapshots — holding the
    /// lock for the entire iteration would risk deadlocks with re-entrant paths.
    pub fn threads_of_process(
        &self,
        pid: crate::proc::pid_table::Pid,
    ) -> alloc::vec::Vec<Arc<KernelThread>> {
        self.table
            .lock()
            .values()
            .filter(|thread| thread.pid() == pid)
            .cloned()
            .collect()
    }
}

/// Global kernel thread table, accessible from any module.
pub static THREAD_TABLE: ThreadTable = ThreadTable::new();
