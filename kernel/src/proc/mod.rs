pub mod loader;
pub mod process;
pub mod thread;

pub use loader::elf::{Elf, LoadedElf};
pub use process::{
    all_processes, find_process, find_processes_by_pgid, next_pid, register_process,
    unregister_process, Process, ProcessId, ProcessState, ProcessTable, PROCESS_TABLE,
};
pub use thread::{Thread, ThreadContext, ThreadId, ThreadState};

use crate::sync::Mutex;
use alloc::sync::Arc;

/// Helper to obtain the currently executing Thread on the active CPU.
pub fn current_thread() -> Option<Arc<Mutex<Thread>>> {
    let cpu_id = crate::arch::cpu_id();
    crate::sched::current_thread_on_cpu(cpu_id)
}

/// Helper to obtain the currently executing Process on the active CPU.
pub fn current_process() -> Option<Arc<Mutex<Process>>> {
    current_thread().and_then(|t| t.lock().process.upgrade())
}
