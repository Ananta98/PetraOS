use crate::proc::process::{Process, ProcessId, ProcessState};
use crate::sched::sched_thread::ThreadId;
use crate::sync::spinlock::Spinlock;
use alloc::collections::BTreeMap;

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
            proc.take_children()
        } else {
            return Err("Process not found");
        };

        // Reparent children to PID 1 (init)
        let init_pid = ProcessId(1);
        for child_pid in children_to_reparent {
            if let Some(child) = self.processes.get_mut(&child_pid) {
                child.set_ppid(Some(init_pid));
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
            child.ppid() == Some(ppid)
        } else {
            return Err("Target process not found");
        };

        if !is_child {
            return Err("Target process is not a child of the caller");
        }

        let is_zombie = self.processes.get(&target_pid)
            .map(|c| c.state() == ProcessState::Zombie)
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
            Ok(child.exit_code().unwrap_or(0))
        } else {
            Err("Failed to remove process")
        }
    }

    /// Add a thread to a process.
    pub fn add_thread_to_process(&mut self, pid: ProcessId, tid: ThreadId) -> Result<(), &'static str> {
        if let Some(proc) = self.processes.get_mut(&pid) {
            proc.add_thread(tid);
            Ok(())
        } else {
            Err("Process not found")
        }
    }

    /// Remove a thread from a process.
    pub fn remove_thread_from_process(&mut self, pid: ProcessId, tid: ThreadId) -> Result<(), &'static str> {
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
