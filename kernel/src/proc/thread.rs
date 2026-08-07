use alloc::sync::{Arc, Weak};
use crate::sync::spinlock::Spinlock;
use crate::sched::{ThreadId, SchedThread};
use crate::ipc::signal::{SigSet, PendingSignals};
use super::process::Process;

/// Represents the execution state of a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Running,
    Sleeping,
    Stopped,
    Zombie,
}

/// The architecture-specific execution context (registers).
#[derive(Debug, Default, Clone)]
pub struct ThreadContext {
    pub rsp: usize,
    pub rip: usize,
    // Add additional registers (e.g. rax, rbx) as needed for the architecture
}

/// Represents an execution context (thread) in the OS.
pub struct Thread {
    /// Unique Thread ID (TID)
    pub tid: ThreadId,
    
    /// The process this thread belongs to (Weak reference to avoid cyclic Arc dependencies)
    pub process: Weak<Spinlock<Process>>,
    
    /// CPU Context (Registers, RSP, RIP)
    pub context: ThreadContext,
    
    /// Scheduler metadata
    pub sched_info: SchedThread,
    
    /// Signal mask (blocked signals for this specific thread)
    pub sig_mask: SigSet,
    
    /// Pending signals directed to this specific thread
    pub pending_signals: PendingSignals,
    
    /// State of the thread
    pub state: ThreadState,
    
    /// Exit code, if the thread has exited
    pub exit_code: Option<u32>,
}

impl Thread {
    pub fn new(tid: ThreadId, process: Weak<Spinlock<Process>>, sched_info: SchedThread) -> Self {
        Self {
            tid,
            process,
            context: ThreadContext::default(),
            sched_info,
            sig_mask: 0,
            pending_signals: PendingSignals::new(),
            state: ThreadState::Running,
            exit_code: None,
        }
    }
}
