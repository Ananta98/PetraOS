//! Scheduler Manager for PetraOS.
//!
//! Provides the top-level `Scheduler` struct coordinating thread scheduling,
//! timer ticks, and hardware context switches via the unified `RunQueue`.

use crate::arch::cpu::msr;
use crate::irq::lock::irq_lock;
use crate::proc::thread::{Thread, ThreadId};
use crate::sched::runqueue::RunQueue;
use crate::sync::futex::FUTEX_MANAGER;
use crate::sync::Mutex;
use alloc::sync::Arc;

/// Scheduler Manager coordinating runnable threads across CPU cores.
pub struct Scheduler {
    /// Mutex guarding the run queue.
    queue: Mutex<RunQueue>,
}

impl Scheduler {
    /// Creates a new `Scheduler`.
    pub const fn new() -> Self {
        Self {
            queue: Mutex::new(RunQueue::new()),
        }
    }

    /// Initializes the run queue.
    pub fn init(&self) {
        // Const initialized via RunQueue::new()
    }

    /// Executes a closure with exclusive access to the run queue.
    #[inline(always)]
    fn with_rq<R>(&self, f: impl FnOnce(&mut RunQueue) -> R) -> R {
        let mut rq = self.queue.lock();
        f(&mut rq)
    }

    /// Obtains the currently executing thread.
    pub fn current_thread(&self) -> Option<Arc<Mutex<Thread>>> {
        self.with_rq(|rq| rq.current())
    }

    /// Obtains the currently executing thread on `_cpu_id`.
    pub fn current_thread_on_cpu(&self, _cpu_id: u32) -> Option<Arc<Mutex<Thread>>> {
        self.current_thread()
    }

    /// Sets the currently executing thread.
    pub fn set_current_thread(&self, thread: Option<Arc<Mutex<Thread>>>) {
        self.with_rq(|rq| rq.set_current(thread));
    }

    /// Sets the currently executing thread on `_cpu_id`.
    pub fn set_current_thread_on_cpu(&self, _cpu_id: u32, thread: Option<Arc<Mutex<Thread>>>) {
        self.set_current_thread(thread);
    }

    /// Enqueues a thread into the run queue.
    pub fn add_thread(&self, thread: Arc<Mutex<Thread>>) {
        self.with_rq(|rq| rq.enqueue(thread));
    }

    /// Removes a thread from the scheduler by its `ThreadId`.
    pub fn remove_thread(&self, tid: ThreadId) -> Option<Arc<Mutex<Thread>>> {
        self.with_rq(|rq| rq.dequeue(tid))
    }

    /// Picks the next runnable thread.
    pub fn pick_next(&self, _cpu_id: u32) -> Option<Arc<Mutex<Thread>>> {
        self.with_rq(|rq| rq.pick_next())
    }

    /// Updates scheduling accounting on timer ticks.
    pub fn tick(&self, _cpu_id: u32, delta_ns: u64) {
        FUTEX_MANAGER
            .lock()
            .check_timeouts(crate::clock::elapsed_ns());

        let should_preempt = self.with_rq(|rq| rq.tick(delta_ns));
        if should_preempt {
            self.schedule(true);
        }
    }

    /// Voluntarily yields the current thread.
    pub fn yield_current(&self, _cpu_id: u32) {
        self.with_rq(|rq| rq.yield_current());
    }

    /// The main scheduling entry point.
    ///
    /// - If `yielding` is `true`: the current thread is returned to its run queue.
    /// - If `yielding` is `false`: the current thread is blocked/exited and removed.
    pub fn schedule(&self, yielding: bool) {
        // Disable interrupts on local CPU during scheduling to prevent interrupt re-entry.
        let irq_guard = irq_lock();

        // ── Critical section: queue manipulation only ──────────────────────
        // Release the run queue Mutex before arch_switch_context to avoid deadlock.
        let (prev, next) = {
            let mut rq = self.queue.lock();
            let prev = rq.current();
            if yielding {
                rq.yield_current();
            } else {
                rq.set_current(None);
            }
            let next = rq.pick_next();
            if let Some(ref t) = next {
                rq.set_current(Some(t.clone()));
            }
            (prev, next)
        };

        match (prev, next) {
            (Some(prev_thread), Some(next_thread)) if Arc::ptr_eq(&prev_thread, &next_thread) => {
                // Same thread re-selected; restore and continue execution without context switch.
                self.with_rq(|rq| rq.set_current(Some(prev_thread)));
                drop(irq_guard);
            }

            (prev_thread, Some(next_thread)) => {
                // Extract target register state for next thread.
                let (next_rsp, next_cr3, next_kstack_top, next_fs_base, next_gs_base) = {
                    let n = next_thread.lock();
                    (
                        n.context.rsp as u64,
                        n.context.cr3 as u64,
                        n.kernel_stack_top(),
                        n.context.fs_base,
                        n.context.gs_base,
                    )
                };

                // Extract previous thread RSP pointer, or null if initial switch.
                let prev_rsp_ptr = match prev_thread {
                    Some(p_arc) => {
                        let mut p = p_arc.lock();
                        p.context.fs_base = msr::read_fs_base();
                        p.context.gs_base = msr::read_kernel_gs_base();
                        &mut p.context.rsp as *mut usize as *mut u64
                    }
                    None => core::ptr::null_mut(),
                };

                // SAFETY: Both context states and page tables are valid; RunQueue lock is released.
                unsafe {
                    crate::arch::arch_switch_context(
                        prev_rsp_ptr,
                        next_rsp,
                        next_cr3,
                        next_kstack_top,
                        next_fs_base,
                        next_gs_base,
                    );
                }

                drop(irq_guard);
            }

            (Some(prev_thread), None) => {
                if yielding {
                    // No other thread runnable; keep current thread running.
                    self.with_rq(|rq| rq.set_current(Some(prev_thread)));
                    drop(irq_guard);
                    return;
                }

                // Current thread blocked/exited and no runnable threads remain; halt until next interrupt.
                drop(irq_guard);
                crate::arch::idle();
            }

            (None, None) => {
                drop(irq_guard);
            }
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Global Scheduler instance.
pub static SCHEDULER: Scheduler = Scheduler::new();

/// Initializes the global scheduler.
pub fn init() {
    SCHEDULER.init();
}

// ── Public Scheduler API ──────────────────────────────────────────────────────

/// Obtains the currently executing thread.
pub fn current_thread() -> Option<Arc<Mutex<Thread>>> {
    SCHEDULER.current_thread()
}

/// Obtains the currently executing thread on `cpu_id`.
pub fn current_thread_on_cpu(cpu_id: u32) -> Option<Arc<Mutex<Thread>>> {
    SCHEDULER.current_thread_on_cpu(cpu_id)
}

/// Sets the currently executing thread.
pub fn set_current_thread(thread: Option<Arc<Mutex<Thread>>>) {
    SCHEDULER.set_current_thread(thread);
}

/// Sets the currently executing thread on `cpu_id`.
pub fn set_current_thread_on_cpu(cpu_id: u32, thread: Option<Arc<Mutex<Thread>>>) {
    SCHEDULER.set_current_thread_on_cpu(cpu_id, thread);
}

/// Enqueues a thread into the scheduler run queue.
pub fn add_thread(thread: Arc<Mutex<Thread>>) {
    SCHEDULER.add_thread(thread);
}

/// Removes a thread from the scheduler run queue by its `ThreadId`.
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
