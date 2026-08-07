use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::sync::spinlock::Spinlock;
use crate::ipc::signal::{SigAction, PendingSignals, MAX_SIGNALS};
use super::thread::Thread;

pub type ProcessId = u64;

/// Represents a Process (Task Group) containing shared resources.
pub struct Process {
    /// Process ID (PID)
    pub pid: ProcessId,
    
    /// Parent Process ID (PPID)
    pub ppid: ProcessId,
    
    // TODO: Add VmSpace, FdTable, and FsContext abstractions
    // pub vmspace: Arc<Spinlock<VmSpace>>,
    // pub fd_table: Arc<Spinlock<FdTable>>,
    // pub fs_context: Arc<Spinlock<FsContext>>,
    
    /// Signal actions (handlers) for the process
    pub sig_actions: [SigAction; MAX_SIGNALS],
    
    /// Pending signals directed to the whole process
    pub pending_signals: PendingSignals,
    
    /// Children processes list
    pub children: Vec<Arc<Spinlock<Process>>>,
    
    /// Threads running in this process
    pub threads: Vec<Arc<Spinlock<Thread>>>,
}

impl Process {
    pub fn new(pid: ProcessId, ppid: ProcessId) -> Self {
        Self {
            pid,
            ppid,
            sig_actions: [Default::default(); MAX_SIGNALS],
            pending_signals: PendingSignals::new(),
            children: Vec::new(),
            threads: Vec::new(),
        }
    }
}
