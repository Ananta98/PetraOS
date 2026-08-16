//! Earliest Eligible Virtual Deadline First (EEVDF) Scheduler.
//!
//! EEVDF schedules threads based on proportional share fairness by tracking:
//! 1. Virtual runtime (`vruntime`): accumulated CPU time scaled by thread weight.
//! 2. System virtual time (`min_vruntime`): monotonic virtual time baseline of the system.
//! 3. Eligibility: a thread $i$ is eligible to run when $v_i \le \text{min\_vruntime}$.
//! 4. Virtual deadline (`vdeadline`): virtual time by which the requested time slice
//!    should complete, computed as $d_i = v_i + \frac{q_i \cdot w_0}{w_i}$.
//!
//! The scheduler always selects the eligible thread with the earliest virtual deadline.

use crate::proc::thread::{Thread, ThreadId, ThreadState};
use crate::sched::nice::NICE_0_WEIGHT;
use crate::sync::spinlock::Spinlock;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;

/// Maximum number of CPUs supported.
pub const MAX_CPUS: usize = 8;

/// Default time slice for threads in nanoseconds (10 ms).
pub const BASE_SLICE_NS: u64 = 10_000_000;

/// The Earliest Eligible Virtual Deadline First (EEVDF) Scheduler.
pub struct Scheduler {
    /// The run queue of ready threads indexed by ThreadId.
    run_queue: BTreeMap<ThreadId, Arc<Spinlock<Thread>>>,

    /// The currently running threads per CPU.
    pub current_threads: [Option<Arc<Spinlock<Thread>>>; MAX_CPUS],

    /// Monotonic system virtual time baseline.
    pub min_vruntime: u64,
}

impl Scheduler {
    /// Creates a new, empty `Scheduler`.
    pub const fn new() -> Self {
        Self {
            run_queue: BTreeMap::new(),
            current_threads: [const { None }; MAX_CPUS],
            min_vruntime: 0,
        }
    }

    /// Adds a thread to the run queue.
    ///
    /// The thread's `vruntime` is normalized against the system `min_vruntime`
    /// to avoid starvation or excessive priority after sleeping. Its virtual deadline
    /// is then computed based on its allocated time slice and weight.
    pub fn add_thread(&mut self, thread: Arc<Spinlock<Thread>>) {
        let mut t_lock = thread.lock();

        // Prevent waking threads from gaining unfair CPU time if they slept for long.
        if t_lock.vruntime < self.min_vruntime {
            t_lock.vruntime = self.min_vruntime;
        }

        // Calculate virtual deadline: d_i = v_i + (q_i * w_0) / w_i
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
        t_lock.vdeadline = t_lock.vruntime.saturating_add(vslice);

        t_lock.state = ThreadState::Ready;
        let tid = t_lock.tid;
        drop(t_lock);

        self.run_queue.insert(tid, thread);
    }

    /// Removes a thread from the run queue by its `ThreadId`.
    pub fn remove_thread(&mut self, tid: ThreadId) -> Option<Arc<Spinlock<Thread>>> {
        self.run_queue.remove(&tid)
    }

    /// Picks the next thread to run for `cpu_id` according to EEVDF rules:
    ///
    /// 1. A thread is **eligible** if its virtual runtime $v_i \le \text{min\_vruntime}$.
    /// 2. If no threads are eligible, advance $\text{min\_vruntime}$ to $\min(v_i)$.
    /// 3. Among eligible threads, pick the one with the earliest virtual deadline ($\min(d_i)$).
    pub fn pick_next(&mut self, cpu_id: u32) -> Option<Arc<Spinlock<Thread>>> {
        if self.run_queue.is_empty() {
            self.current_threads[cpu_id as usize] = None;
            return None;
        }

        // Step 1: Find min vruntime across all queued threads to advance system virtual time if needed.
        let mut min_rq_vruntime = u64::MAX;
        for thread in self.run_queue.values() {
            let t_lock = thread.lock();
            if t_lock.vruntime < min_rq_vruntime {
                min_rq_vruntime = t_lock.vruntime;
            }
        }

        if min_rq_vruntime != u64::MAX && self.min_vruntime < min_rq_vruntime {
            self.min_vruntime = min_rq_vruntime;
        }

        // Step 2: Select the eligible thread with the earliest virtual deadline.
        // Eligibility: vruntime <= self.min_vruntime
        // Metric: (vdeadline, vruntime, tid)
        let mut best_tid: Option<ThreadId> = None;
        let mut best_deadline = u64::MAX;
        let mut best_vruntime = u64::MAX;

        for (&tid, thread) in self.run_queue.iter() {
            let t_lock = thread.lock();
            let is_eligible = t_lock.vruntime <= self.min_vruntime;

            if is_eligible {
                if t_lock.vdeadline < best_deadline
                    || (t_lock.vdeadline == best_deadline && t_lock.vruntime < best_vruntime)
                {
                    best_deadline = t_lock.vdeadline;
                    best_vruntime = t_lock.vruntime;
                    best_tid = Some(tid);
                }
            }
        }

        // Fallback: If no thread was eligible due to rounding/bounds, pick the one with earliest deadline.
        if best_tid.is_none() {
            for (&tid, thread) in self.run_queue.iter() {
                let t_lock = thread.lock();
                if t_lock.vdeadline < best_deadline
                    || (t_lock.vdeadline == best_deadline && t_lock.vruntime < best_vruntime)
                {
                    best_deadline = t_lock.vdeadline;
                    best_vruntime = t_lock.vruntime;
                    best_tid = Some(tid);
                }
            }
        }

        let selected_tid = best_tid?;
        let selected_thread = self.run_queue.remove(&selected_tid)?;

        let mut t_lock = selected_thread.lock();
        t_lock.state = ThreadState::Running;
        let selected_vruntime = t_lock.vruntime;
        drop(t_lock);

        // Advance system virtual time monotonically to selected thread's vruntime
        if selected_vruntime > self.min_vruntime {
            self.min_vruntime = selected_vruntime;
        }

        self.current_threads[cpu_id as usize] = Some(selected_thread.clone());
        Some(selected_thread)
    }

    /// Updates the `vruntime` of the currently running thread on `cpu_id`.
    ///
    /// `delta_ns` is the time elapsed since the last tick (e.g. 10 ms = 10_000_000 ns).
    pub fn tick(&mut self, cpu_id: u32, delta_ns: u64) {
        if let Some(thread) = &self.current_threads[cpu_id as usize] {
            let mut t_lock = thread.lock();

            let weight = if t_lock.weight > 0 {
                t_lock.weight
            } else {
                NICE_0_WEIGHT
            };
            let vruntime_delta = (delta_ns * NICE_0_WEIGHT as u64) / weight as u64;

            t_lock.vruntime = t_lock.vruntime.saturating_add(vruntime_delta);
        }
    }

    /// Voluntarily yield the CPU for `cpu_id`.
    pub fn yield_current(&mut self, cpu_id: u32) {
        if let Some(thread) = self.current_threads[cpu_id as usize].take() {
            let mut t_lock = thread.lock();
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

            // Advance vruntime and virtual deadline so other queued threads run first
            t_lock.vruntime = t_lock.vruntime.max(self.min_vruntime).saturating_add(vslice);
            t_lock.vdeadline = t_lock.vruntime.saturating_add(vslice);
            t_lock.state = ThreadState::Ready;
            let tid = t_lock.tid;
            drop(t_lock);

            self.run_queue.insert(tid, thread);
        }
    }

    /// Blocks the current thread on `cpu_id` (removes it from CPU without putting back in run queue).
    pub fn block_current(&mut self, cpu_id: u32) {
        self.current_threads[cpu_id as usize] = None;
    }
}
