//! Modular Object-Oriented Per-CPU Scheduler Subsystem for PetraOS.
//!
//! Features a two-tier scheduling hierarchy per CPU core:
//! 1. **Real-Time (RT) Class**: Fixed-priority (`SCHED_FIFO`) and round-robin (`SCHED_RR`)
//!    with 100 distinct priority levels (0..=99). Managed via lockless O(1) MPSC queues.
//!    Real-time tasks strictly preempt fair tasks with zero lock contention on enqueue.
//! 2. **Fair (EEVDF) Class**: Proportional-share Earliest Eligible Virtual Deadline First
//!    for normal tasks (`SCHED_OTHER` / `SCHED_NORMAL`), scaled by nice values (-20..19).
//!
//! Scheduling decisions avoid holding inner thread locks during queue traversal to
//! completely prevent lock-inversion deadlocks. Architecture context switching is
//! cleanly abstracted into `crate::arch::sched`.

pub mod fair;
pub mod nice;
pub mod percpu;
pub mod policy;
pub mod preempt;
pub mod realtime;

use crate::arch::cpu::msr;

use crate::proc::thread::{Thread, ThreadId};
use crate::sync::Mutex;
use alloc::sync::Arc;
use alloc::vec::Vec;

pub use fair::{BASE_SLICE_NS, EevdfEntity, EevdfScheduler, FairClassRq};
pub use nice::{MAX_NICE, MIN_NICE, NICE_0_WEIGHT, Nice, nice_to_weight};
pub use percpu::PerCpuRunQueue;
pub use policy::{
    DEFAULT_RR_QUANTUM_NS, MAX_RT_PRIO, MIN_RT_PRIO, RT_PRIO_COUNT, RtPriority, SchedPolicy,
};
pub use preempt::{MAX_PREEMPT_CPUS, can_preempt, preempt_count, preempt_disable, preempt_enable};
pub use realtime::{RtClassRq, RtRunQueue};

/// Object-Oriented Per-CPU Scheduler manager.
///
/// Holds dynamically-allocated per-CPU run queues sized to the detected hardware CPU count.
pub struct PerCpuScheduler {
    /// Dynamically-managed per-CPU run queues.
    queues: Mutex<Vec<Arc<Mutex<PerCpuRunQueue>>>>,
}

impl PerCpuScheduler {
    /// Creates a new `PerCpuScheduler`. Run queues are populated lazily or via `init()`.
    pub const fn new() -> Self {
        Self {
            queues: Mutex::new(Vec::new()),
        }
    }

    /// Explicitly pre-initializes run queues for all detected CPUs.
    pub fn init(&self) {
        let total = crate::arch::cpu_count() as usize;
        let mut q = self.queues.lock();
        if q.len() < total {
            for i in q.len()..total {
                q.push(Arc::new(Mutex::new(PerCpuRunQueue::new(i as u32))));
            }
        }
    }

    /// Obtains (or lazily initializes) the run queue for `cpu_id`.
    #[inline]
    fn queue_for_cpu(&self, cpu_id: u32) -> Arc<Mutex<PerCpuRunQueue>> {
        let mut q = self.queues.lock();
        let idx = cpu_id as usize;
        if idx >= q.len() {
            let target_len = (idx + 1).max(crate::arch::cpu_count() as usize);
            for i in q.len()..target_len {
                q.push(Arc::new(Mutex::new(PerCpuRunQueue::new(i as u32))));
            }
        }
        q[idx].clone()
    }

    /// Obtains the currently executing thread on `cpu_id`.
    pub fn current_thread_on_cpu(&self, cpu_id: u32) -> Option<Arc<Mutex<Thread>>> {
        let rq = self.queue_for_cpu(cpu_id);
        crate::arch::without_interrupts(|| rq.lock().current())
    }

    /// Sets the currently executing thread on `cpu_id`.
    pub fn set_current_thread_on_cpu(&self, cpu_id: u32, thread: Option<Arc<Mutex<Thread>>>) {
        let rq = self.queue_for_cpu(cpu_id);
        crate::arch::without_interrupts(|| {
            rq.lock().set_current(thread);
        });
    }

    /// Enqueues a thread into the appropriate per-CPU run queue based on its policy and CPU affinity.
    pub fn add_thread(&self, thread: Arc<Mutex<Thread>>) {
        let total_cpus = crate::arch::cpu_count();
        let current_cpu = crate::arch::cpu_id();
        let affinity = thread.lock().affinity;

        let target_cpu = if (affinity & (1u64 << current_cpu)) != 0 {
            current_cpu
        } else {
            let mut chosen = current_cpu;
            for i in 0..total_cpus {
                if (affinity & (1u64 << i)) != 0 {
                    chosen = i;
                    break;
                }
            }
            chosen
        };

        let rq = self.queue_for_cpu(target_cpu);

        crate::arch::without_interrupts(|| {
            rq.lock().enqueue(thread);
        });
    }

    /// Removes a thread from all scheduler run queues by its `ThreadId`.
    pub fn remove_thread(&self, tid: ThreadId) -> Option<Arc<Mutex<Thread>>> {
        let queues: Vec<Arc<Mutex<PerCpuRunQueue>>> = {
            let mut q = self.queues.lock();
            let total = crate::arch::cpu_count() as usize;
            if q.len() < total {
                for i in q.len()..total {
                    q.push(Arc::new(Mutex::new(PerCpuRunQueue::new(i as u32))));
                }
            }
            q.clone()
        };

        crate::arch::without_interrupts(|| {
            for rq in queues {
                if let Some(thread) = rq.lock().dequeue(tid) {
                    return Some(thread);
                }
            }
            None
        })
    }

    /// Picks the next thread to run on `cpu_id` according to the class hierarchy:
    ///
    /// 1. **Real-Time (RT)**: Highest priority available in `RtClassRq`.
    /// 2. **Fair (EEVDF)**: Earliest virtual deadline among eligible threads in `FairClassRq`.
    pub fn pick_next(&self, cpu_id: u32) -> Option<Arc<Mutex<Thread>>> {
        let rq = self.queue_for_cpu(cpu_id);
        crate::arch::without_interrupts(|| rq.lock().pick_next())
    }

    /// Updates scheduling accounting on timer ticks.
    pub fn tick(&self, cpu_id: u32, delta_ns: u64) {
        let rq = self.queue_for_cpu(cpu_id);
        let should_preempt = crate::arch::without_interrupts(|| rq.lock().tick(delta_ns));

        if should_preempt {
            self.schedule(true);
        }
    }

    /// Voluntarily yields the current thread on `cpu_id`.
    pub fn yield_current(&self, cpu_id: u32) {
        let rq = self.queue_for_cpu(cpu_id);
        crate::arch::without_interrupts(|| {
            rq.lock().yield_current();
        });
    }

    /// The main scheduling entry point.
    ///
    /// - If `yielding` is `true`: the current thread is returned to its run queue.
    /// - If `yielding` is `false`: the current thread is blocked/exited and removed.
    pub fn schedule(&self, yielding: bool) {
        // Disable interrupts on local CPU during scheduling to prevent interrupt re-entry
        let saved_flags = crate::arch::disable_interrupts();
        let cpu_id = crate::arch::cpu_id();
        let rq_arc = self.queue_for_cpu(cpu_id);

        let mut rq = rq_arc.lock();
        let prev = rq.current();

        if yielding {
            rq.yield_current();
        } else {
            rq.set_current(None);
        }

        let next = rq.pick_next();

        match (prev, next) {
            (Some(prev_thread), Some(next_thread)) => {
                if Arc::ptr_eq(&prev_thread, &next_thread) {
                    rq.set_current(Some(prev_thread));
                    drop(rq);
                    if saved_flags {
                        crate::arch::enable_interrupts();
                    }
                    return;
                }

                rq.set_current(Some(next_thread.clone()));

                // Extract next thread execution state
                let (next_rsp, next_cr3, next_kstack_top, next_fs_base) = {
                    let n = next_thread.lock();
                    (
                        n.context.rsp as u64,
                        n.context.cr3 as u64,
                        n.kernel_stack_top(),
                        n.context.fs_base,
                    )
                };

                // Save prev thread state and get pointer to save new RSP
                let prev_rsp_ptr = {
                    let mut p = prev_thread.lock();
                    p.context.fs_base = msr::read_fs_base();
                    &mut p.context.rsp as *mut usize as *mut u64
                };

                drop(rq);

                // SAFETY: Context switch between two valid thread execution stacks.
                unsafe {
                    crate::arch::arch_switch_context(
                        prev_rsp_ptr,
                        next_rsp,
                        next_cr3,
                        next_kstack_top,
                        next_fs_base,
                    );
                }

                if saved_flags {
                    crate::arch::enable_interrupts();
                }
            }

            (None, Some(next_thread)) => {
                rq.set_current(Some(next_thread.clone()));

                let (next_rsp, next_cr3, next_kstack_top, next_fs_base) = {
                    let n = next_thread.lock();
                    (
                        n.context.rsp as u64,
                        n.context.cr3 as u64,
                        n.kernel_stack_top(),
                        n.context.fs_base,
                    )
                };

                drop(rq);

                // SAFETY: Context switch into initial thread stack.
                unsafe {
                    crate::arch::arch_switch_context(
                        core::ptr::null_mut(),
                        next_rsp,
                        next_cr3,
                        next_kstack_top,
                        next_fs_base,
                    );
                }
            }

            (Some(prev_thread), None) => {
                if yielding {
                    // If yielding and no other thread is runnable, keep running the current thread
                    rq.set_current(Some(prev_thread));
                    drop(rq);
                    if saved_flags {
                        crate::arch::enable_interrupts();
                    }
                    return;
                }

                drop(rq);
                // If blocked/exited and no runnable threads remain, wait for next interrupt
                if saved_flags {
                    crate::arch::enable_interrupts();
                }
                crate::arch::idle();
            }

            (None, None) => {
                drop(rq);
                if saved_flags {
                    crate::arch::enable_interrupts();
                }
            }
        }
    }
}

impl Default for PerCpuScheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Global Per-CPU Scheduler instance.
pub static SCHEDULER: PerCpuScheduler = PerCpuScheduler::new();

/// Initializes the global per-CPU scheduler for all detected CPUs.
pub fn init() {
    SCHEDULER.init();
}

// ── Public Scheduler API ──────────────────────────────────────────────────────

/// Obtains the currently executing thread on `cpu_id`.
pub fn current_thread_on_cpu(cpu_id: u32) -> Option<Arc<Mutex<Thread>>> {
    SCHEDULER.current_thread_on_cpu(cpu_id)
}

/// Sets the currently executing thread on `cpu_id`.
pub fn set_current_thread_on_cpu(cpu_id: u32, thread: Option<Arc<Mutex<Thread>>>) {
    SCHEDULER.set_current_thread_on_cpu(cpu_id, thread);
}

/// Enqueues a thread into the appropriate per-CPU scheduling class.
pub fn add_thread(thread: Arc<Mutex<Thread>>) {
    SCHEDULER.add_thread(thread);
}

/// Removes a thread from the scheduler run queues by its `ThreadId`.
pub fn remove_thread(tid: ThreadId) -> Option<Arc<Mutex<Thread>>> {
    SCHEDULER.remove_thread(tid)
}

/// Picks the next thread to run on `cpu_id`.
pub fn pick_next(cpu_id: u32) -> Option<Arc<Mutex<Thread>>> {
    SCHEDULER.pick_next(cpu_id)
}

/// Updates scheduling accounting on timer ticks.
pub fn tick(cpu_id: u32, delta_ns: u64) {
    SCHEDULER.tick(cpu_id, delta_ns);
}

/// Voluntarily yields the current thread on `cpu_id`.
pub fn yield_current(cpu_id: u32) {
    SCHEDULER.yield_current(cpu_id);
}

/// The main scheduling entry point.
pub fn schedule(yielding: bool) {
    SCHEDULER.schedule(yielding);
}
