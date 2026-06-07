use crate::sched::task::TaskId;
use crate::sync::spinlock::Spinlock;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::mm::AddrSpace;
use crate::arch::x86_64::paging::X86_64PageTable;

/// Opaque, unique identifier for a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

/// The execution state of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Active,
    Zombie,
}

/// A process control block (PCB) tracking process resources and relationships.
pub struct Process {
    pid: ProcessId,
    ppid: Option<ProcessId>,
    state: ProcessState,
    threads: Vec<TaskId>,
    children: Vec<ProcessId>,
    exit_code: Option<i32>,
    pub addr_space: Option<AddrSpace<X86_64PageTable>>,
}

impl Process {
    /// Create a new process.
    pub fn new(pid: ProcessId, ppid: Option<ProcessId>) -> Self {
        Self {
            pid,
            ppid,
            state: ProcessState::Active,
            threads: Vec::new(),
            children: Vec::new(),
            exit_code: None,
            addr_space: None,
        }
    }

    /// Returns the process ID.
    pub fn pid(&self) -> ProcessId {
        self.pid
    }

    /// Returns the parent process ID.
    pub fn ppid(&self) -> Option<ProcessId> {
        self.ppid
    }

    /// Returns the current state of the process.
    pub fn state(&self) -> ProcessState {
        self.state
    }

    /// Returns the list of threads owned by this process.
    pub fn threads(&self) -> &[TaskId] {
        &self.threads
    }

    /// Returns the list of child process IDs.
    pub fn children(&self) -> &[ProcessId] {
        &self.children
    }

    /// Returns the process exit code, if terminated.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Associate a thread with this process.
    pub fn add_thread(&mut self, tid: TaskId) {
        if !self.threads.contains(&tid) {
            self.threads.push(tid);
        }
    }

    /// Disassociate a thread from this process.
    pub fn remove_thread(&mut self, tid: TaskId) {
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

/// The global process manager tracking process lifecycle, state, and hierarchy.
pub struct ProcessManager {
    processes: BTreeMap<ProcessId, Process>,
    next_pid: u64,
}

impl ProcessManager {
    /// Create a new process manager.
    pub const fn new() -> Self {
        Self {
            processes: BTreeMap::new(),
            next_pid: 1,
        }
    }

    /// Create a new process, setting up the parent/child hierarchy.
    pub fn create_process(&mut self, ppid: Option<ProcessId>) -> ProcessId {
        let pid = ProcessId(self.next_pid);
        self.next_pid += 1;

        let proc = Process::new(pid, ppid);
        self.processes.insert(pid, proc);

        if let Some(parent_id) = ppid {
            if let Some(parent) = self.processes.get_mut(&parent_id) {
                parent.add_child(pid);
            }
        }

        pid
    }

    /// Terminate a process, marking it as a Zombie, and reparenting its children to init (PID 1).
    pub fn exit_process(&mut self, pid: ProcessId, exit_code: i32) -> Result<(), &'static str> {
        let children_to_reparent = if let Some(proc) = self.processes.get_mut(&pid) {
            proc.exit(exit_code);
            let children = proc.children.clone();
            proc.children.clear();
            children
        } else {
            return Err("Process not found");
        };

        // Reparent children to PID 1 (init)
        let init_pid = ProcessId(1);
        for child_pid in children_to_reparent {
            if let Some(child) = self.processes.get_mut(&child_pid) {
                child.ppid = Some(init_pid);
                if let Some(init_proc) = self.processes.get_mut(&init_pid) {
                    init_proc.add_child(child_pid);
                }
            }
        }

        Ok(())
    }

    /// Reap a zombie child process, retrieving its exit code and cleaning up its resources.
    pub fn wait_pid(&mut self, ppid: ProcessId, target_pid: ProcessId) -> Result<i32, &'static str> {
        let is_child = if let Some(child) = self.processes.get(&target_pid) {
            child.ppid == Some(ppid)
        } else {
            return Err("Target process not found");
        };

        if !is_child {
            return Err("Target process is not a child of the caller");
        }

        let is_zombie = self.processes.get(&target_pid)
            .map(|c| c.state == ProcessState::Zombie)
            .unwrap_or(false);

        if !is_zombie {
            return Err("Target process is not a zombie");
        }

        // Remove from parent's child list
        if let Some(parent) = self.processes.get_mut(&ppid) {
            parent.remove_child(target_pid);
        }

        // Remove the process from the global registry
        if let Some(child) = self.processes.remove(&target_pid) {
            Ok(child.exit_code.unwrap_or(0))
        } else {
            Err("Failed to remove process")
        }
    }

    /// Add a thread to a process.
    pub fn add_thread_to_process(&mut self, pid: ProcessId, tid: TaskId) -> Result<(), &'static str> {
        if let Some(proc) = self.processes.get_mut(&pid) {
            proc.add_thread(tid);
            Ok(())
        } else {
            Err("Process not found")
        }
    }

    /// Remove a thread from a process.
    pub fn remove_thread_from_process(&mut self, pid: ProcessId, tid: TaskId) -> Result<(), &'static str> {
        if let Some(proc) = self.processes.get_mut(&pid) {
            proc.remove_thread(tid);
            Ok(())
        } else {
            Err("Process not found")
        }
    }

    /// Get a reference to a process in the registry.
    pub fn get_process(&self, pid: ProcessId) -> Option<&Process> {
        self.processes.get(&pid)
    }

    /// Get a mutable reference to a process in the registry.
    pub fn get_process_mut(&mut self, pid: ProcessId) -> Option<&mut Process> {
        self.processes.get_mut(&pid)
    }
}

/// The global process manager singleton.
pub static PROCESS_MANAGER: Spinlock<ProcessManager> = Spinlock::new(ProcessManager::new());
