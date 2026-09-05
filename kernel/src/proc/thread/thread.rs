use super::tid::ThreadId;
use crate::arch::cpu::stack::KernelStack;
use crate::arch::sched::ThreadContext;
use crate::ipc::signal::{PendingSignals, SigSet};
use crate::ipc::signal::{SIG_BLOCK, SIG_SETMASK, SIG_UNBLOCK, SIGKILL, SIGSTOP};
use crate::proc::process::Process;
use crate::sched::nice::Nice;
use crate::sched::policy::{DEFAULT_RR_QUANTUM_NS, RtPriority, SchedPolicy};
use crate::sync::Mutex;
use alloc::string::String;
use alloc::sync::{Arc, Weak};

/// Default requested time slice for threads in nanoseconds (10 ms).
pub const DEFAULT_THREAD_SLICE_NS: u64 = 10_000_000;

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
    pub process: Weak<Mutex<Process>>,

    /// CPU Context (Registers, RSP, RIP)
    pub context: ThreadContext,

    /// Dynamically allocated kernel stack for Ring 0 transitions and context switches
    pub kernel_stack: Option<KernelStack>,

    /// Scheduling policy (Fair, Fifo, RoundRobin)
    pub sched_policy: SchedPolicy,

    /// Real-time scheduling priority (0..=99)
    pub rt_priority: RtPriority,

    /// Remaining time quantum in nanoseconds for `SCHED_RR`
    pub rr_remaining_ns: u64,

    /// Accumulated virtual runtime in nanoseconds (EEVDF)
    pub vruntime: u64,

    /// Virtual deadline in nanoseconds (EEVDF)
    pub vdeadline: u64,

    /// Requested time slice in nanoseconds
    pub slice_ns: u64,

    /// Thread scheduling nice value ([-20, 19])
    pub nice: Nice,

    /// Thread weight for proportional sharing (higher weight = more CPU time)
    pub weight: u32,

    /// Signal mask (blocked signals for this specific thread)
    pub sig_mask: SigSet,

    /// Pending signals directed to this specific thread
    pub pending_signals: PendingSignals,

    /// CPU affinity mask (bitmask of allowed CPUs, default !0 = all CPUs)
    pub affinity: u64,

    /// State of the thread
    pub state: ThreadState,

    /// Exit code, if the thread has exited
    pub exit_code: Option<u32>,
}

impl Thread {
    pub fn new(tid: ThreadId, name: String, weight: u32, process: Weak<Mutex<Process>>) -> Self {
        let nice = Nice::default();
        let effective_weight = if weight > 0 { weight } else { nice.weight() };
        Self {
            tid,
            name,
            process,
            context: ThreadContext::default(),
            kernel_stack: None,
            sched_policy: SchedPolicy::Fair,
            rt_priority: RtPriority::DEFAULT,
            rr_remaining_ns: DEFAULT_RR_QUANTUM_NS,
            vruntime: 0,
            vdeadline: 0,
            slice_ns: DEFAULT_THREAD_SLICE_NS,
            nice,
            weight: effective_weight,
            sig_mask: 0,
            pending_signals: PendingSignals::new(),
            affinity: !0u64,
            state: ThreadState::Creating,
            exit_code: None,
        }
    }

    /// Returns the kernel stack top virtual address if allocated.
    pub fn kernel_stack_top(&self) -> u64 {
        self.kernel_stack
            .as_ref()
            .map(|s| s.top().as_u64())
            .unwrap_or(0)
    }

    /// Sets the thread kernel stack.
    pub fn set_kernel_stack(&mut self, kstack: KernelStack) {
        self.kernel_stack = Some(kstack);
    }

    /// Configure execution context for a forked child thread.
    pub fn setup_fork_context(
        &mut self,
        kstack: KernelStack,
        child_rsp: u64,
        cr3: usize,
        fs_base: u64,
        gs_base: u64,
        sig_mask: SigSet,
    ) {
        self.context.rsp = child_rsp as usize;
        self.context.cr3 = cr3;
        self.context.rflags = 0x202;
        self.context.fs_base = fs_base;
        self.context.gs_base = gs_base;
        self.sig_mask = sig_mask;
        self.kernel_stack = Some(kstack);
        self.state = ThreadState::Ready;
    }

    /// Sets the thread nice value and updates its associated CPU weight.
    pub fn set_nice(&mut self, nice: Nice) {
        self.nice = nice;
        self.weight = nice.weight();
    }

    /// Sets the scheduling policy and real-time priority.
    pub fn set_scheduler_policy(&mut self, policy: SchedPolicy, rt_prio: RtPriority) {
        self.sched_policy = policy;
        self.rt_priority = rt_prio;
        self.rr_remaining_ns = DEFAULT_RR_QUANTUM_NS;
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
    pub fn unblock(thread: Arc<Mutex<Thread>>) {
        let mut t = thread.lock();
        if t.state == ThreadState::Sleeping {
            t.state = ThreadState::Ready;
            drop(t);
            crate::sched::add_thread(thread);
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
        let unblockable = (1 << (SIGKILL - 1)) | (1 << (SIGSTOP - 1));
        let set = set & !unblockable;

        match how {
            SIG_BLOCK => self.sig_mask |= set,
            SIG_UNBLOCK => self.sig_mask &= !set,
            SIG_SETMASK => self.sig_mask = set,
            _ => return Err("Invalid sigprocmask how argument"),
        }
        Ok(old_mask)
    }
}
