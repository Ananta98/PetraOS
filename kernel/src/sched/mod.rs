pub mod fair;
pub mod nice;

use crate::arch::cpu::context::{switch_context, switch_context_to};


use crate::sync::spinlock::Spinlock;
use alloc::sync::Arc;

pub use fair::{BASE_SLICE_NS, MAX_CPUS, Scheduler};
pub use nice::{nice_to_weight, Nice, MAX_NICE, MIN_NICE, NICE_0_WEIGHT};

/// Global EEVDF Scheduler instance
pub static SCHEDULER: Spinlock<Scheduler> = Spinlock::new(Scheduler::new());

/// The main scheduling routine.
/// If `yielding` is true, the current thread is placed back in the run queue.
/// If `yielding` is false, the current thread is blocked (or exiting) and is not put back.
pub fn schedule(yielding: bool) {
    // Disable interrupts on the local CPU while holding SCHEDULER lock to prevent deadlock
    let saved_flags = crate::arch::disable_interrupts();

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
                drop(sched);
                if saved_flags {
                    crate::arch::enable_interrupts();
                }
                return; // Nothing to do
            }
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
            // First ever thread switch (from kmain)
            let next_rsp = next.lock().context.rsp as u64;
            drop(sched);
            // SAFETY: Switching CPU context to initial thread.
            unsafe { switch_context_to(next_rsp) };
        }
        (Some(_), None) => {
            // No runnable threads. Drop scheduler lock then idle permanently.
            // idle() is divergent (loop { hlt }), so we never return here and
            // never fall back into the dead syscall frame.
            drop(sched);
            if saved_flags {
                crate::arch::enable_interrupts();
            }
            crate::arch::idle();
        }

        (None, None) => {
            // Do nothing, idle or still booting.
            drop(sched);
            if saved_flags {
                crate::arch::enable_interrupts();
            }
            return;
        }
    }
}
