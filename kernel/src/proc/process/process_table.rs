use super::pid::ProcessId;
use super::process::Process;
use crate::sync::Mutex;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Global process table structure managing active system processes.
pub struct ProcessTable {
    table: Mutex<BTreeMap<ProcessId, Arc<Mutex<Process>>>>,
}

impl ProcessTable {
    /// Create a new empty process table.
    pub const fn new() -> Self {
        Self {
            table: Mutex::new(BTreeMap::new()),
        }
    }

    /// Register a process in the table.
    pub fn register(&self, proc: Arc<Mutex<Process>>) {
        let pid = proc.lock().pid;
        self.table.lock().insert(pid, proc);
    }

    /// Unregister a process from the table by its `ProcessId`.
    pub fn unregister(&self, pid: ProcessId) -> Option<Arc<Mutex<Process>>> {
        self.table.lock().remove(&pid)
    }

    /// Find a process in the table by its `ProcessId`.
    pub fn find(&self, pid: ProcessId) -> Option<Arc<Mutex<Process>>> {
        self.table.lock().get(&pid).cloned()
    }

    /// Find all processes belonging to a specific Process Group ID (`pgid`).
    pub fn find_by_pgid(&self, pgid: ProcessId) -> Vec<Arc<Mutex<Process>>> {
        let table = self.table.lock();
        table
            .values()
            .filter(|proc| proc.lock().pgid == pgid)
            .cloned()
            .collect()
    }
}

/// Global static instance of the ProcessTable.
pub static PROCESS_TABLE: ProcessTable = ProcessTable::new();

/// Register a new active process in the global process table.
pub fn register_process(proc: Arc<Mutex<Process>>) {
    PROCESS_TABLE.register(proc);
}

/// Unregister a process from the global process table upon termination/reap.
pub fn unregister_process(pid: ProcessId) {
    PROCESS_TABLE.unregister(pid);
}

/// Find a process in the global process table by its `ProcessId`.
pub fn find_process(pid: ProcessId) -> Option<Arc<Mutex<Process>>> {
    PROCESS_TABLE.find(pid)
}

/// Find all processes belonging to a process group.
pub fn find_processes_by_pgid(pgid: ProcessId) -> Vec<Arc<Mutex<Process>>> {
    PROCESS_TABLE.find_by_pgid(pgid)
}
