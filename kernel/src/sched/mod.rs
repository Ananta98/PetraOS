pub mod fair;
pub mod stats;

use alloc::sync::Arc;
use crate::arch::cpu::context::{switch_context, switch_context_to};
use crate::sync::spinlock::Spinlock;

pub use fair::{Scheduler, MAX_CPUS, NICE_0_WEIGHT};
pub use stats::SchedulerStats;

/// Global CFS Scheduler instance
pub static SCHEDULER: Spinlock<Scheduler> = Spinlock::new(Scheduler::new());

/// The main scheduling routine.
/// If `yielding` is true, the current thread is placed back in the run queue.
/// If `yielding` is false, the current thread is blocked (or exiting) and is not put back.
pub fn schedule(yielding: bool) {
    let cpu_id = crate::arch::cpu_id();
    let mut sched = SCHEDULER.lock();
    let prev_thread = sched.current_threads[cpu_id as usize].clone();

    if yielding {
        sched.yield_current(cpu_id);
    } else {
        sched.block_current(cpu_id);
    }

    let next_thread = sched.pick_next(cpu_id);

    match (prev_thread, next_thread) {
        (Some(prev), Some(next)) => {
            if Arc::ptr_eq(&prev, &next) {
                return; // Nothing to do
            }
            sched.stats.inc_context_switches();
            // Get raw pointers
            let prev_rsp_ptr = {
                let mut p = prev.lock();
                &mut p.context.rsp as *mut usize as *mut u64
            };
            let next_rsp = {
                let n = next.lock();
                n.context.rsp as u64
            };

            drop(sched);
            // SAFETY: Switching CPU context between valid thread stack pointers.
            unsafe { switch_context(prev_rsp_ptr, next_rsp) };
        }
        (None, Some(next)) => {
            sched.stats.inc_context_switches();
            // First ever thread switch (from kmain)
            let next_rsp = next.lock().context.rsp as u64;
            drop(sched);
            // SAFETY: Switching CPU context to initial thread.
            unsafe { switch_context_to(next_rsp) };
        }
        (Some(_), None) => {
            // No runnable threads. We should halt/idle.
            panic!("No runnable threads!");
        }
        (None, None) => {
            // Do nothing, idle or still booting.
            return;
        }
    }
}
