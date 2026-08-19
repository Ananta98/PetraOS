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
                p.context.fs_base = crate::arch::cpu::msr::read_fs_base();
                &mut p.context.rsp as *mut usize as *mut u64
            };
            let (next_rsp, next_cr3, next_kstack_top, next_fs_base) = {
                let n = next.lock();
                (
                    n.context.rsp as u64,
                    n.context.cr3 as u64,
                    n.kernel_stack_top(),
                    n.context.fs_base,
                )
            };

            drop(sched);

            // Switch page directory if changing address spaces
            if next_cr3 != 0 {
                let active_cr3 = crate::arch::active_address_space_root();
                if next_cr3 != active_cr3 {
                    // SAFETY: next_cr3 is a valid PML4 physical root address for the target process.
                    unsafe {
                        crate::arch::set_address_space_root(next_cr3);
                    }
                }
            }

            // Restore IA32_FS_BASE for TLS context
            crate::arch::cpu::msr::write_fs_base(next_fs_base);

            // Update TSS RSP0 and CpuLocal kernel stack pointer for Ring 3 transitions
            if next_kstack_top != 0 {
                crate::arch::cpu::tss::set_rsp0(next_kstack_top);
            }

            // SAFETY: Switching CPU context between valid thread stack pointers.
            unsafe { switch_context(prev_rsp_ptr, next_rsp) };

            if saved_flags {
                crate::arch::enable_interrupts();
            }
        }
        (None, Some(next)) => {
            // First ever thread switch (from kmain)
            let (next_rsp, next_cr3, next_kstack_top, next_fs_base) = {
                let n = next.lock();
                (
                    n.context.rsp as u64,
                    n.context.cr3 as u64,
                    n.kernel_stack_top(),
                    n.context.fs_base,
                )
            };
            drop(sched);

            if next_cr3 != 0 {
                let active_cr3 = crate::arch::active_address_space_root();
                if next_cr3 != active_cr3 {
                    // SAFETY: next_cr3 is a valid PML4 physical root address for the target process.
                    unsafe {
                        crate::arch::set_address_space_root(next_cr3);
                    }
                }
            }

            crate::arch::cpu::msr::write_fs_base(next_fs_base);

            if next_kstack_top != 0 {
                crate::arch::cpu::tss::set_rsp0(next_kstack_top);
            }

            // SAFETY: Switching CPU context to initial thread.
            unsafe { switch_context_to(next_rsp) };
        }
        (Some(prev), None) => {
            if yielding {
                // If yielding and no other thread is ready, continue running the current thread.
                sched.current_threads[cpu_id as usize] = Some(prev);
                drop(sched);
                if saved_flags {
                    crate::arch::enable_interrupts();
                }
                return;
            }
            // If blocked/exited and no runnable threads, drop scheduler lock and idle until next interrupt.
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
