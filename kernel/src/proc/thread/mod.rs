pub mod thread;
pub mod tid;

pub use crate::arch::cpu::context::ThreadContext;
pub use thread::{Thread, ThreadState};
pub use tid::{ThreadId, next_tid};

/// Called automatically by `thread_bootstrapper` in `Switch.S` if the thread entry function returns.
#[unsafe(no_mangle)]
pub extern "C" fn thread_exit() -> ! {
    let cpu_id = crate::arch::cpu_id();
    let thread = {
        let sched = crate::sched::SCHEDULER.lock();
        sched.current_threads[cpu_id as usize].clone()
    };

    if let Some(t) = thread {
        t.lock().exit(0);
    } else {
        panic!("thread_exit called with no current thread!");
    }

    unreachable!("Thread exited but didn't switch context");
}
