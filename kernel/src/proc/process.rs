use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use alloc::string::String;
use crate::sync::spinlock::Spinlock;
use crate::ipc::signal::{SigAction, PendingSignals, MAX_SIGNALS};
use crate::mm::vmm::AddrSpace;
use crate::arch::paging::ArchPageTable;
use crate::sched::ThreadId;
use super::thread::Thread;

pub type ProcessId = u64;

/// State of a process
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Creating,
    Ready,
    Running,
    Stopped,
    Zombie,
}

/// Represents a Process (Task Group) containing shared resources.
pub struct Process {
    /// Process ID (PID)
    pub pid: ProcessId,
    
    /// Parent Process ID (PPID)
    pub ppid: ProcessId,
    
    /// Process state
    pub state: ProcessState,
    
    /// Virtual Address Space
    pub address_space: Arc<Spinlock<AddrSpace<ArchPageTable>>>,
    
    /// Exit code when process terminates
    pub exit_code: Option<i32>,
    
    // TODO: Add FdTable, and FsContext abstractions
    // pub fd_table: Arc<Spinlock<FdTable>>,
    // pub fs_context: Arc<Spinlock<FsContext>>,
    
    /// Signal actions (handlers) for the process
    pub sig_actions: [SigAction; MAX_SIGNALS],
    
    /// Pending signals directed to the whole process
    pub pending_signals: PendingSignals,
    
    /// Children processes list
    pub children: BTreeMap<ProcessId, Arc<Spinlock<Process>>>,
    
    /// Threads running in this process
    pub threads: BTreeMap<ThreadId, Arc<Spinlock<Thread>>>,
}

impl Process {
    pub fn new(pid: ProcessId, ppid: ProcessId, address_space: Arc<Spinlock<AddrSpace<ArchPageTable>>>) -> Self {
        Self {
            pid,
            ppid,
            state: ProcessState::Creating,
            address_space,
            exit_code: None,
            sig_actions: [Default::default(); MAX_SIGNALS],
            pending_signals: PendingSignals::new(),
            children: BTreeMap::new(),
            threads: BTreeMap::new(),
        }
    }
    
    /// Execute an executable file.
    pub fn execute(&mut self, _file_name: &str, _argc: usize, _argv: *const *const u8, _envp: *const *const u8) -> Result<(), &'static str> {
        // TODO: Load ELF, setup main thread, and copy arguments to user stack.
        Err("execute not implemented")
    }
    
    /// Wait for a child process to exit.
    pub fn wait(&mut self, _pid: ProcessId) -> Result<i32, &'static str> {
        // TODO: Implement process wait logic
        Err("wait not implemented")
    }
    
    /// Terminate the process.
    pub fn exit(&mut self, status: i32) {
        self.state = ProcessState::Zombie;
        self.exit_code = Some(status);
        // TODO: Terminate threads, reparent children, wake up waiting parent
    }
}
