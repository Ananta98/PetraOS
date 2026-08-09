use super::cmdline::CommandLine;
use super::pid::{ProcessId, next_pid};
use crate::arch::paging::ArchPageTable;
use crate::ipc::signal::{MAX_SIGNALS, PendingSignals, SigAction};
use crate::mm::PageTable;
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
}
