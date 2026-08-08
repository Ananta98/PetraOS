use alloc::sync::{Arc, Weak};
use alloc::string::String;
use crate::sync::spinlock::Spinlock;
use crate::sched::{ThreadId, SchedThread};
use crate::ipc::signal::{SigSet, PendingSignals};
use super::process::Process;

/// Represents the execution state of a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Creating,
    Ready,
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
    
    /// Thread name
    pub name: String,
    
    /// The process this thread belongs to (Weak reference to avoid cyclic Arc dependencies)
    pub process: Weak<Spinlock<Process>>,
    
    /// CPU Context (Registers, RSP, RIP)
    pub context: ThreadContext,
    
    /// Scheduler metadata
    pub sched_info: SchedThread,
    
    /// Current priority
    pub priority: u8,
    
    /// Base priority (before any priority donations)
    pub base_priority: u8,
    
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
    pub fn new(tid: ThreadId, name: String, priority: u8, process: Weak<Spinlock<Process>>, sched_info: SchedThread) -> Self {
        Self {
            tid,
            name,
            process,
            context: ThreadContext::default(),
            sched_info,
            priority,
            base_priority: priority,
            sig_mask: 0,
            pending_signals: PendingSignals::new(),
            state: ThreadState::Creating,
            exit_code: None,
        }
    }
    
    /// Yield the CPU to another thread.
    pub fn yield_cpu() {
        // TODO: Implement thread yield (set state to Ready and call scheduler)
    }
    
    /// Block the current thread.
    pub fn block(&mut self) {
        self.state = ThreadState::Sleeping;
        // TODO: Call scheduler
    }
    
    /// Unblock the thread (transition from Sleeping to Ready).
    pub fn unblock(&mut self) {
        if self.state == ThreadState::Sleeping {
            self.state = ThreadState::Ready;
            // TODO: Add back to ready queue
        }
    }
    
    /// Terminate the thread.
    pub fn exit(&mut self, status: u32) {
        self.state = ThreadState::Zombie;
        self.exit_code = Some(status);
        // TODO: Clean up resources and call scheduler
    }
}
