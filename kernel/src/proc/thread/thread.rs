use super::tid::ThreadId;
use crate::arch::cpu::context::ThreadContext;
use crate::ipc::signal::{PendingSignals, SigSet};
use crate::proc::process::Process;
use crate::sync::spinlock::Spinlock;
use alloc::string::String;
use alloc::sync::{Arc, Weak};

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

    /// Accumulated virtual runtime in nanoseconds (CFS)
    pub vruntime: u64,

    /// Thread weight for CFS (higher weight = more CPU time)
    pub weight: u32,

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
    pub fn new(tid: ThreadId, name: String, weight: u32, process: Weak<Spinlock<Process>>) -> Self {
        Self {
            tid,
            name,
            process,
            context: ThreadContext::default(),
            vruntime: 0,
            weight,
            sig_mask: 0,
            pending_signals: PendingSignals::new(),
            state: ThreadState::Creating,
            exit_code: None,
        }
    }

    /// Yield the CPU to another thread.
    pub fn yield_cpu() {
        crate::sched::schedule(true);
    }

    /// Block the current thread.
    pub fn block(&mut self) {
        self.state = ThreadState::Sleeping;
        crate::sched::schedule(false);
    }

    /// Unblock the thread (transition from Sleeping to Ready).
    pub fn unblock(thread: Arc<Spinlock<Thread>>) {
        let mut t = thread.lock();
        if t.state == ThreadState::Sleeping {
            t.state = ThreadState::Ready;
            drop(t);
            let saved_flags = crate::arch::disable_interrupts();
            crate::sched::SCHEDULER.lock().add_thread(thread);
            if saved_flags {
                crate::arch::enable_interrupts();
            }
        }
    }

    /// Terminate the thread.
    pub fn exit(&mut self, status: u32) {
        self.state = ThreadState::Zombie;
        self.exit_code = Some(status);
        // Remove from CPU and never return
        crate::sched::schedule(false);
    }

    /// Update thread signal mask (sigprocmask semantics).
    pub fn update_sigmask(&mut self, how: i32, set: SigSet) -> Result<SigSet, &'static str> {
        let old_mask = self.sig_mask;
        // SIGKILL and SIGSTOP cannot be blocked
        let unblockable = (1 << (crate::ipc::signal::SIGKILL - 1)) | (1 << (crate::ipc::signal::SIGSTOP - 1));
        let set = set & !unblockable;

        match how {
            crate::ipc::signal::SIG_BLOCK => self.sig_mask |= set,
            crate::ipc::signal::SIG_UNBLOCK => self.sig_mask &= !set,
            crate::ipc::signal::SIG_SETMASK => self.sig_mask = set,
            _ => return Err("Invalid sigprocmask how argument"),
        }
        Ok(old_mask)
    }
}

