pub mod loader;
pub mod process;
pub mod thread;

pub use loader::elf::{Elf, LoadedElf};
pub use process::{
    find_process, find_processes_by_pgid, next_pid, register_process, unregister_process,
    Process, ProcessId, ProcessState, ProcessTable, PROCESS_TABLE,
};
pub use thread::{thread_exit, Thread, ThreadContext, ThreadId, ThreadState};

use crate::sync::spinlock::Spinlock;
use alloc::sync::Arc;

/// Helper to obtain the currently executing Thread on the active CPU.
pub fn current_thread() -> Option<Arc<Spinlock<Thread>>> {
    let cpu_id = crate::arch::cpu_id();
    crate::arch::without_interrupts(|| {
        let sched = crate::sched::SCHEDULER.lock();
        sched.current_threads[cpu_id as usize].clone()
    })
}

/// Helper to obtain the currently executing Process on the active CPU.
pub fn current_process() -> Option<Arc<Spinlock<Process>>> {
    current_thread().and_then(|t| t.lock().process.upgrade())
}