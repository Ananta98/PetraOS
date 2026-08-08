use super::cmdline::CommandLine;
use super::pid::{next_pid, ProcessId};
use crate::arch::paging::ArchPageTable;
use crate::ipc::signal::{PendingSignals, SigAction, MAX_SIGNALS};
use crate::mm::vmm::AddrSpace;
use crate::proc::thread::{Thread, ThreadId, ThreadState};
use crate::sync::spinlock::Spinlock;
use alloc::collections::BTreeMap;
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

    /// Clone the current process.
    pub fn fork(parent: Arc<Spinlock<Process>>) -> Result<Arc<Spinlock<Process>>, &'static str> {
        let p_lock = parent.lock();
        let child_pid = next_pid();

        let mut child_proc = Process::new(
            child_pid,
            p_lock.pid,
            p_lock.address_space.clone(),
        );
        child_proc.cmdline = p_lock.cmdline.clone();

        let child = Arc::new(Spinlock::new(child_proc));

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
}
