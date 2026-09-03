pub mod thread;
pub mod tid;

pub use crate::arch::cpu::context::ThreadContext;
pub use thread::{Thread, ThreadState};
pub use tid::{next_tid, ThreadId};

/// Called when a thread finishes execution to clean up and reschedule.
#[unsafe(no_mangle)]
pub extern "C" fn thread_exit() -> ! {
    let cpu_id = crate::arch::cpu_id();
    let thread = crate::sched::current_thread_on_cpu(cpu_id);

    if let Some(t) = thread {
        t.lock().exit(0);
    } else {
        panic!("thread_exit called with no current thread!");
    }

    unreachable!("Thread exited but didn't switch context");
}
