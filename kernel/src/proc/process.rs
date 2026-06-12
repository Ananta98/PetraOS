use crate::arch::x86_64::paging::X86_64PageTable;
use crate::fs::fd::FdTable;
use crate::mm::AddrSpace;
use crate::sched::sched_thread::ThreadId;
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Opaque, unique identifier for a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

/// The execution state of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Active,
    Blocked,
    Zombie,
}

/// A process control block (PCB) tracking process resources and relationships.
pub struct Process {
    pid: ProcessId,
    ppid: Option<ProcessId>,
    state: ProcessState,
    threads: Vec<ThreadId>,
    children: Vec<ProcessId>,
    exit_code: Option<i32>,
    addr_space: Option<AddrSpace<X86_64PageTable>>,
    /// Per-process file descriptor table.
    pub fd_table: Arc<FdTable>,
}

impl Process {
    /// Create a new process.
    pub fn new(pid: ProcessId, ppid: Option<ProcessId>) -> Self {
        Self {
            pid,
            ppid,
            state: ProcessState::Ready,
            threads: Vec::new(),
            children: Vec::new(),
            exit_code: None,
            addr_space: None,
            fd_table: Arc::new(FdTable::new()),
        }
    }

    /// Set up standard file descriptors (0, 1, 2) referencing the console device.
    pub fn setup_std_fds(&self, console_file: Arc<crate::fs::File>) {
        self.fd_table.setup_std_fds(console_file);
    }

    /// Returns the process ID.
    pub fn pid(&self) -> ProcessId {
        self.pid
    }

    /// Returns the parent process ID.
    pub fn ppid(&self) -> Option<ProcessId> {
        self.ppid
    }

    /// Sets the parent process ID.
    pub fn set_ppid(&mut self, ppid: Option<ProcessId>) {
        self.ppid = ppid;
    }

    /// Returns the current state of the process.
    pub fn state(&self) -> ProcessState {
        self.state
    }

    /// Sets the state of the process.
    pub fn set_state(&mut self, state: ProcessState) {
        self.state = state;
    }

    /// Returns the list of threads owned by this process.
    pub fn threads(&self) -> &[ThreadId] {
        &self.threads
    }

    /// Returns the list of child process IDs.
    pub fn children(&self) -> &[ProcessId] {
        &self.children
    }

    /// Extract and clear the list of child processes.
    pub fn take_children(&mut self) -> Vec<ProcessId> {
        core::mem::take(&mut self.children)
    }

    /// Get a reference to the process's address space.
    pub fn addr_space(&self) -> Option<&AddrSpace<X86_64PageTable>> {
        self.addr_space.as_ref()
    }

    /// Set the process's address space.
    pub fn set_addr_space(&mut self, addr_space: AddrSpace<X86_64PageTable>) {
        self.addr_space = Some(addr_space);
    }

    /// Returns the process exit code, if terminated.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Associate a thread with this process.
    pub fn add_thread(&mut self, tid: ThreadId) {
        if !self.threads.contains(&tid) {
            self.threads.push(tid);
            // If the process was in the Ready state, transition to Active.
            if self.state == ProcessState::Ready {
                self.state = ProcessState::Active;
            }
        }
    }

    /// Disassociate a thread from this process.
    pub fn remove_thread(&mut self, tid: ThreadId) {
        self.threads.retain(|&t| t != tid);
    }

    /// Add a child process to this process.
    pub fn add_child(&mut self, child_pid: ProcessId) {
        if !self.children.contains(&child_pid) {
            self.children.push(child_pid);
        }
    }

    /// Remove a child process from this process.
    pub fn remove_child(&mut self, child_pid: ProcessId) {
        self.children.retain(|&c| c != child_pid);
    }

    /// Terminate this process and transition to a Zombie state.
    pub fn exit(&mut self, exit_code: i32) {
        self.state = ProcessState::Zombie;
        self.exit_code = Some(exit_code);
    }
}
