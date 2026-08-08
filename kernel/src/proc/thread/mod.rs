pub mod thread;
pub mod tid;

pub use crate::arch::cpu::context::ThreadContext;
pub use thread::{Thread, ThreadState};
pub use tid::{next_tid, ThreadId};

/// Called automatically by `thread_bootstrapper` in `Switch.S` if the thread entry function returns.
#[unsafe(no_mangle)]
pub extern "C" fn thread_exit() -> ! {
    let cpu_id = crate::arch::cpu_id();
    let thread = {
        let saved_flags = crate::arch::disable_interrupts();
        let sched = crate::sched::SCHEDULER.lock();
        let current = sched.current_threads[cpu_id as usize].clone();
        drop(sched);
        if saved_flags {
            crate::arch::enable_interrupts();
        }
        current
    };

    if let Some(t) = thread {
        t.lock().exit(0);
    } else {
        panic!("thread_exit called with no current thread!");
    }

    unreachable!("Thread exited but didn't switch context");
}
