//! Modular Hybrid Scheduler Subsystem for PetraOS.
//!
//! Features a two-tier scheduling hierarchy:
//! 1. **Real-Time (RT) Class**: Fixed-priority (`SCHED_FIFO`) and round-robin (`SCHED_RR`)
//!    with 100 distinct priority levels (0..=99). Managed via lockless O(1) MPSC queues.
//!    Real-time tasks strictly preempt fair tasks with zero lock contention on enqueue.
//! 2. **Fair (EEVDF) Class**: Proportional-share Earliest Eligible Virtual Deadline First
//!    for normal tasks (`SCHED_OTHER` / `SCHED_NORMAL`), scaled by nice values (-20..19).
//!
//! Scheduling decisions avoid holding inner thread locks during queue traversal to
//! completely prevent lock-inversion deadlocks.

pub mod fair;
pub mod nice;
pub mod policy;
pub mod realtime;

use crate::arch::cpu::context::{switch_context, switch_context_to};
use crate::arch::cpu::{msr, tss};
use crate::proc::thread::{Thread, ThreadId, ThreadState};
use crate::sync::spinlock::Spinlock;
use alloc::sync::Arc;

pub use fair::{BASE_SLICE_NS, EevdfScheduler};
pub use nice::{MAX_NICE, MIN_NICE, NICE_0_WEIGHT, Nice, nice_to_weight};
pub use policy::{
    DEFAULT_RR_QUANTUM_NS, MAX_RT_PRIO, MIN_RT_PRIO, RT_PRIO_COUNT, RtPriority, SchedPolicy,
};
pub use realtime::{RtNode, RtRunQueue};

/// Maximum number of CPUs supported.
pub const MAX_CPUS: usize = 8;

/// Lockless real-time run queues per CPU.
static RT_QUEUES: [RtRunQueue; MAX_CPUS] = [
    RtRunQueue::new(),
    RtRunQueue::new(),
    RtRunQueue::new(),
    RtRunQueue::new(),
    RtRunQueue::new(),
    RtRunQueue::new(),
    RtRunQueue::new(),
    RtRunQueue::new(),
];

/// Global EEVDF fair scheduler instance.
pub static FAIR_SCHEDULER: Spinlock<EevdfScheduler> = Spinlock::new(EevdfScheduler::new());

/// The currently executing thread per CPU.
static CURRENT_THREADS: [Spinlock<Option<Arc<Spinlock<Thread>>>>; MAX_CPUS] = [
    Spinlock::new(None),
    Spinlock::new(None),
    Spinlock::new(None),
    Spinlock::new(None),
    Spinlock::new(None),
    Spinlock::new(None),
    Spinlock::new(None),
    Spinlock::new(None),
];

/// Obtains the currently executing thread on `cpu_id`.
pub fn current_thread_on_cpu(cpu_id: u32) -> Option<Arc<Spinlock<Thread>>> {
    if (cpu_id as usize) < MAX_CPUS {
        crate::arch::without_interrupts(|| CURRENT_THREADS[cpu_id as usize].lock().clone())
    } else {
        None
    }
}

/// Sets the currently executing thread on `cpu_id`.
pub fn set_current_thread_on_cpu(cpu_id: u32, thread: Option<Arc<Spinlock<Thread>>>) {
    if (cpu_id as usize) < MAX_CPUS {
        crate::arch::without_interrupts(|| {
            *CURRENT_THREADS[cpu_id as usize].lock() = thread;
        });
    }
}

/// Enqueues a thread into the appropriate scheduling class based on its policy.
///
/// Real-time threads are enqueued locklessly into `RT_QUEUES`.
/// Fair threads are enqueued into `FAIR_SCHEDULER`.
pub fn add_thread(thread: Arc<Spinlock<Thread>>) {
    let (policy, rt_prio) = {
        let t_lock = thread.lock();
        (t_lock.sched_policy, t_lock.rt_priority)
    };

    if policy.is_realtime() {
        let cpu_id = crate::arch::cpu_id() as usize;
        let target_cpu = if cpu_id < MAX_CPUS { cpu_id } else { 0 };
        RT_QUEUES[target_cpu].enqueue(thread, rt_prio);
    } else {
        crate::arch::without_interrupts(|| {
            FAIR_SCHEDULER.lock().add_thread(thread);
        });
    }
}

/// Removes a thread from the scheduler run queues by its `ThreadId`.
pub fn remove_thread(tid: ThreadId) -> Option<Arc<Spinlock<Thread>>> {
    crate::arch::without_interrupts(|| FAIR_SCHEDULER.lock().remove_thread(tid))
}

/// Picks the next thread to run on `cpu_id` according to the class hierarchy:
///
/// 1. **Real-Time (RT)**: Highest priority available in lockless `RT_QUEUES`.
/// 2. **Fair (EEVDF)**: Earliest virtual deadline among eligible threads in `FAIR_SCHEDULER`.
pub fn pick_next(cpu_id: u32) -> Option<Arc<Spinlock<Thread>>> {
    // Restrict scheduling to BSP CPU 0 until secondary AP core thread stacks are configured
    if cpu_id != 0 {
        return None;
    }

    let cpu_idx = cpu_id as usize;

    // 1. Try real-time run queue first (strictly preempts fair scheduling)
    if let Some(rt_thread) = RT_QUEUES[cpu_idx].dequeue_highest() {
        let mut t_lock = rt_thread.lock();
        t_lock.state = ThreadState::Running;
        drop(t_lock);
        return Some(rt_thread);
    }

    // 2. Fall back to EEVDF fair scheduler
    FAIR_SCHEDULER.lock().pick_next()
}

/// Updates scheduling accounting on timer ticks.
///
/// For `SCHED_RR`: decrements time quantum and triggers preemption if expired.
/// For `SCHED_OTHER`: advances virtual runtime in EEVDF.
pub fn tick(cpu_id: u32, delta_ns: u64) {
    if let Some(thread) = current_thread_on_cpu(cpu_id) {
        let mut t_lock = thread.lock();
        match t_lock.sched_policy {
            SchedPolicy::RoundRobin => {
                if t_lock.rr_remaining_ns <= delta_ns {
                    t_lock.rr_remaining_ns = DEFAULT_RR_QUANTUM_NS;
                    drop(t_lock);
                    // Quantum expired, yield CPU
                    schedule(true);
                } else {
                    t_lock.rr_remaining_ns -= delta_ns;
                }
            }
            SchedPolicy::Fair => {
                let weight = if t_lock.weight > 0 {
                    t_lock.weight
                } else {
                    NICE_0_WEIGHT
                };
                let vruntime_delta = (delta_ns * NICE_0_WEIGHT as u64) / weight as u64;
                t_lock.vruntime = t_lock.vruntime.saturating_add(vruntime_delta);
            }
            SchedPolicy::Fifo => {
                // SCHED_FIFO runs until voluntary yield, block, or preemption by higher RT priority.
            }
        }
    }
}

/// Voluntarily yields the current thread on `cpu_id`.
pub fn yield_current(cpu_id: u32) {
    if let Some(thread) = current_thread_on_cpu(cpu_id) {
        set_current_thread_on_cpu(cpu_id, None);

        let mut t_lock = thread.lock();
        let policy = t_lock.sched_policy;
        let rt_prio = t_lock.rt_priority;

        match policy {
            SchedPolicy::Fair => {
                let weight = if t_lock.weight > 0 {
                    t_lock.weight
                } else {
                    NICE_0_WEIGHT
                };
                let slice_ns = if t_lock.slice_ns > 0 {
                    t_lock.slice_ns
                } else {
                    BASE_SLICE_NS
                };
                let vslice = (slice_ns * NICE_0_WEIGHT as u64) / weight as u64;

                let min_vr = FAIR_SCHEDULER.lock().min_vruntime;
                t_lock.vruntime = t_lock.vruntime.max(min_vr).saturating_add(vslice);
                t_lock.vdeadline = t_lock.vruntime.saturating_add(vslice);
                t_lock.state = ThreadState::Ready;
                drop(t_lock);

                FAIR_SCHEDULER.lock().add_thread(thread);
            }
            SchedPolicy::RoundRobin => {
                t_lock.rr_remaining_ns = DEFAULT_RR_QUANTUM_NS;
                t_lock.state = ThreadState::Ready;
                drop(t_lock);

                RT_QUEUES[cpu_id as usize].enqueue(thread, rt_prio);
            }
            SchedPolicy::Fifo => {
                t_lock.state = ThreadState::Ready;
                drop(t_lock);

                RT_QUEUES[cpu_id as usize].enqueue(thread, rt_prio);
            }
        }
    }
}

/// The main scheduling entry point.
///
/// - If `yielding` is `true`: the current thread is returned to its run queue.
/// - If `yielding` is `false`: the current thread is blocked/exited and removed.
pub fn schedule(yielding: bool) {
    // Disable interrupts on local CPU during scheduling to prevent interrupt re-entry
    let saved_flags = crate::arch::disable_interrupts();
    let cpu_id = crate::arch::cpu_id();

    let prev_thread = current_thread_on_cpu(cpu_id);

    if yielding {
        yield_current(cpu_id);
    } else {
        set_current_thread_on_cpu(cpu_id, None);
    }

    let next_thread = pick_next(cpu_id);

    match (prev_thread, next_thread) {
        (Some(prev), Some(next)) => {
            if Arc::ptr_eq(&prev, &next) {
                set_current_thread_on_cpu(cpu_id, Some(prev));
                if saved_flags {
                    crate::arch::enable_interrupts();
                }
                return;
            }

            set_current_thread_on_cpu(cpu_id, Some(next.clone()));

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

            // Switch address space if needed
            if next_cr3 != 0 {
                let active_cr3 = crate::arch::active_address_space_root();
                if next_cr3 != active_cr3 {
                    // SAFETY: next_cr3 is a verified PML4 address space root.
                    unsafe {
                        crate::arch::set_address_space_root(next_cr3);
                    }
                }
            }

            // Restore TLS base register
            msr::write_fs_base(next_fs_base);

            // Update TSS RSP0 for Ring 3 transitions
            if next_kstack_top != 0 {
                tss::set_rsp0(next_kstack_top);
            }

            // SAFETY: Context switch between two valid thread execution stacks.
            unsafe { switch_context(prev_rsp_ptr, next_rsp) };

            if saved_flags {
                crate::arch::enable_interrupts();
            }
        }

        (None, Some(next)) => {
            set_current_thread_on_cpu(cpu_id, Some(next.clone()));

            let (next_rsp, next_cr3, next_kstack_top, next_fs_base) = {
                let n = next.lock();
                (
                    n.context.rsp as u64,
                    n.context.cr3 as u64,
                    n.kernel_stack_top(),
                    n.context.fs_base,
                )
            };

            if next_cr3 != 0 {
                let active_cr3 = crate::arch::active_address_space_root();
                if next_cr3 != active_cr3 {
                    // SAFETY: next_cr3 is a verified PML4 address space root.
                    unsafe {
                        crate::arch::set_address_space_root(next_cr3);
                    }
                }
            }

            msr::write_fs_base(next_fs_base);

            if next_kstack_top != 0 {
                tss::set_rsp0(next_kstack_top);
            }

            // SAFETY: Context switch into initial thread stack.
            unsafe { switch_context_to(next_rsp) };
        }

        (Some(prev), None) => {
            if yielding {
                // If yielding and no other thread is runnable, keep running the current thread
                set_current_thread_on_cpu(cpu_id, Some(prev));
                if saved_flags {
                    crate::arch::enable_interrupts();
                }
                return;
            }

            // If blocked/exited and no runnable threads remain, wait for next interrupt
            if saved_flags {
                crate::arch::enable_interrupts();
            }
            crate::arch::idle();
        }

        (None, None) => {
            if saved_flags {
                crate::arch::enable_interrupts();
            }
        }
    }
}
