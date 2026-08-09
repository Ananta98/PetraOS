use super::cmdline::CommandLine;
use super::pid::{ProcessId, next_pid};
use crate::arch::paging::ArchPageTable;
use crate::ipc::signal::{MAX_SIGNALS, PendingSignals, SigAction};
use crate::mm::PageTable;
use crate::mm::vmm::AddrSpace;
use crate::proc::thread::{Thread, ThreadId, ThreadState};
use crate::sync::spinlock::Spinlock;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;

/// State of a process.
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

    /// Command line arguments and environment variables
    pub cmdline: CommandLine,

    /// Exit code when process terminates
    pub exit_code: Option<i32>,

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
    pub fn new(
        pid: ProcessId,
        ppid: ProcessId,
        address_space: Arc<Spinlock<AddrSpace<ArchPageTable>>>,
    ) -> Self {
        Self {
            pid,
            ppid,
            state: ProcessState::Creating,
            address_space,
            cmdline: CommandLine::default(),
            exit_code: None,
            sig_actions: [Default::default(); MAX_SIGNALS],
            pending_signals: PendingSignals::new(),
            children: BTreeMap::new(),
            threads: BTreeMap::new(),
        }
    }

    /// Execute an executable file with arguments and environment.
    pub fn execute(
        &mut self,
        file_name: &str,
        argc: usize,
        argv: *const *const u8,
        envp: *const *const u8,
    ) -> Result<(), &'static str> {
        let cmdline = unsafe { CommandLine::from_raw(argc, argv, envp)? };
        log::info!(
            "Executing process '{}' (PID {}) with {} arg(s)",
            file_name,
            self.pid,
            cmdline.argc()
        );
        self.cmdline = cmdline;
        self.state = ProcessState::Running;
        Ok(())
    }

    /// Clone the current process (POSIX fork semantics).
    pub fn fork(parent: Arc<Spinlock<Process>>) -> Result<Arc<Spinlock<Process>>, &'static str> {
        let p_lock = parent.lock();
        let child_pid = next_pid();

        // 1. Deep-copy virtual address space (PML4 page table and allocated physical pages)
        let parent_addr_space = p_lock.address_space.lock();
        let child_addr_space = parent_addr_space
            .clone()
            .map_err(|_| "Failed to clone address space for child process")?;
        drop(parent_addr_space);

        let child_addr_space_arc = Arc::new(Spinlock::new(child_addr_space));
        let child_cr3 = child_addr_space_arc.lock().page_table().root().as_u64() as usize;

        // 2. Initialize child process structure
        let mut child_proc = Process::new(child_pid, p_lock.pid, child_addr_space_arc);
        child_proc.cmdline = p_lock.cmdline.clone();
        child_proc.sig_actions = p_lock.sig_actions;
        child_proc.state = p_lock.state;

        let child = Arc::new(Spinlock::new(child_proc));

        // 3. Clone process threads and register in scheduler
        let mut child_threads = BTreeMap::new();
        for (&_tid, thread_arc) in p_lock.threads.iter() {
            let t_lock = thread_arc.lock();
            let child_tid = crate::proc::thread::next_tid();

            let mut child_thread = Thread::new(
                child_tid,
                t_lock.name.clone(),
                t_lock.weight,
                Arc::downgrade(&child),
            );

            child_thread.context = t_lock.context;
            child_thread.context.cr3 = child_cr3;
            child_thread.sig_mask = t_lock.sig_mask;
            child_thread.state = t_lock.state;

            let c_thread_arc = Arc::new(Spinlock::new(child_thread));
            child_threads.insert(child_tid, c_thread_arc.clone());

            if t_lock.state == ThreadState::Ready || t_lock.state == ThreadState::Running {
                let saved_flags = crate::arch::disable_interrupts();
                crate::sched::SCHEDULER.lock().add_thread(c_thread_arc);
                if saved_flags {
                    crate::arch::enable_interrupts();
                }
            }
        }

        child.lock().threads = child_threads;

        drop(p_lock);
        parent.lock().children.insert(child_pid, child.clone());

        Ok(child)
    }

    /// Wait for a child process to exit.
    pub fn wait(&mut self, pid: ProcessId) -> Result<i32, &'static str> {
        if !self.children.contains_key(&pid) {
            return Err("Child not found");
        }

        loop {
            let child = self.children.get(&pid).unwrap().clone();
            let c_lock = child.lock();
            if c_lock.state == ProcessState::Zombie {
                let exit_code = c_lock.exit_code.unwrap_or(0);
                drop(c_lock);
                self.children.remove(&pid);
                return Ok(exit_code);
            }
            drop(c_lock);

            Thread::yield_cpu();
        }
    }

    /// Terminate the process.
    pub fn exit(&mut self, status: i32) {
        self.state = ProcessState::Zombie;
        self.exit_code = Some(status);

        // Terminate all threads
        let saved_flags = crate::arch::disable_interrupts();
        for (_, thread) in self.threads.iter_mut() {
            let mut t_lock = thread.lock();
            t_lock.state = ThreadState::Zombie;
            crate::sched::SCHEDULER.lock().remove_thread(t_lock.tid);
        }
        if saved_flags {
            crate::arch::enable_interrupts();
        }
        self.threads.clear();
    }

    /// Update signal action for a given signal number (sigaction semantics).
    pub fn sigaction(&mut self, sig: u8, act: Option<SigAction>) -> Result<SigAction, &'static str> {
        if sig == 0 || sig > 64 {
            return Err("Invalid signal number");
        }
        if crate::ipc::signal::is_uncatchable(sig) && act.is_some() {
            return Err("Cannot catch or ignore SIGKILL or SIGSTOP");
        }

        let sig_idx = (sig - 1) as usize;
        let old_action = self.sig_actions[sig_idx];
        if let Some(new_action) = act {
            self.sig_actions[sig_idx] = new_action;
        }

        Ok(old_action)
    }

    /// Send a POSIX signal to this process.
    pub fn send_signal(&mut self, sig: u8) -> Result<(), &'static str> {
        if sig == 0 || sig > 64 {
            return Err("Invalid signal number");
        }

        // Special immediate signals
        if sig == crate::ipc::signal::SIGKILL {
            log::info!("Process PID {} terminated by SIGKILL", self.pid);
            self.exit(128 + sig as i32);
            return Ok(());
        }

        if sig == crate::ipc::signal::SIGSTOP {
            self.state = ProcessState::Stopped;
            for (_, thread_arc) in self.threads.iter() {
                let mut t = thread_arc.lock();
                t.state = ThreadState::Stopped;
            }
            return Ok(());
        }

        if sig == crate::ipc::signal::SIGCONT {
            if self.state == ProcessState::Stopped {
                self.state = ProcessState::Running;
            }
            for (_, thread_arc) in self.threads.iter() {
                let mut t = thread_arc.lock();
                if t.state == ThreadState::Stopped {
                    t.state = ThreadState::Ready;
                }
            }
            return Ok(());
        }

        // Standard signal delivery: add to process pending queue
        self.pending_signals.add(sig);

        // Wake up sleeping threads so they can process the signal (UNIX EINTR behavior)
        for (_, thread_arc) in self.threads.iter() {
            Thread::unblock(thread_arc.clone());
        }

        Ok(())
    }

    /// Evaluate and handle pending signals for a process thread prior to user return.
    pub fn handle_pending_signals(
        &mut self,
        thread: &mut Thread,
        frame: &mut crate::arch::syscall::syscall::SyscallFrame,
    ) {
        let sig_opt = self
            .pending_signals
            .dequeue(thread.sig_mask)
            .or_else(|| thread.pending_signals.dequeue(thread.sig_mask));

        let sig = match sig_opt {
            Some(s) => s,
            None => return,
        };

        let action = self.sig_actions[(sig - 1) as usize];

        if action.handler == crate::ipc::signal::SIG_IGN {
            return;
        }

        if action.handler == crate::ipc::signal::SIG_DFL {
            match crate::ipc::signal::default_action(sig) {
                crate::ipc::signal::SignalDefaultAction::Terminate
                | crate::ipc::signal::SignalDefaultAction::CoreDump => {
                    log::info!("Process PID {} killed by signal {}", self.pid, sig);
                    self.exit(128 + sig as i32);
                }
                crate::ipc::signal::SignalDefaultAction::Stop => {
                    self.state = ProcessState::Stopped;
                    thread.state = ThreadState::Stopped;
                }
                crate::ipc::signal::SignalDefaultAction::Continue => {
                    self.state = ProcessState::Running;
                    thread.state = ThreadState::Running;
                }
                crate::ipc::signal::SignalDefaultAction::Ignore => {}
            }
            return;
        }

        // Custom signal handler execution frame setup
        let old_mask = thread.sig_mask;
        thread.sig_mask |= action.mask | (1 << (sig - 1));

        unsafe {
            let _ = crate::arch::signal::setup_signal_frame(frame, sig, &action, old_mask);
        }
    }
}

/// Automated Kernel Integration Test for POSIX Signals
pub fn test_signals() {
    log::info!("── Running POSIX Signal Integration Test ──");
    let mut pending = PendingSignals::new();
    pending.add(crate::ipc::signal::SIGUSR1);
    pending.add(crate::ipc::signal::SIGINT);

    assert!(pending.has(crate::ipc::signal::SIGUSR1));
    assert!(pending.has(crate::ipc::signal::SIGINT));
    assert!(!pending.has(crate::ipc::signal::SIGKILL));

    // Test dequeuing with signal mask
    let blocked_mask = 1 << (crate::ipc::signal::SIGUSR1 - 1);
    let dequeued = pending.dequeue(blocked_mask);
    assert_eq!(dequeued, Some(crate::ipc::signal::SIGINT));

    // Verify uncatchable check
    assert!(crate::ipc::signal::is_uncatchable(crate::ipc::signal::SIGKILL));
    assert!(crate::ipc::signal::is_uncatchable(crate::ipc::signal::SIGSTOP));
    assert!(!crate::ipc::signal::is_uncatchable(crate::ipc::signal::SIGUSR1));

    log::info!("✔ TEST PASSED: POSIX Signal logic verified successfully!");
}

