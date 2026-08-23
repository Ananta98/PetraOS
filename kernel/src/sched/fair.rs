//! Earliest Eligible Virtual Deadline First (EEVDF) Scheduler.
//!
//! Implements proportional-share fair scheduling based on:
//! 1. Virtual runtime (`vruntime`): accumulated CPU execution time scaled by thread weight.
//! 2. System virtual time (`min_vruntime`): monotonic virtual time baseline of the system.
//! 3. Eligibility: a thread $i$ is eligible to run when $v_i \le \text{min\_vruntime}$.
//! 4. Virtual deadline (`vdeadline`): virtual time by which the allocated time slice
//!    should complete, computed as $d_i = v_i + \frac{q_i \cdot w_0}{w_i}$.
//!
//! Scheduling decisions are made without acquiring inner thread locks during queue traversal,
//! avoiding lock inversion deadlocks and minimizing scheduling latency.

use crate::proc::thread::{Thread, ThreadId, ThreadState};
use crate::sched::nice::NICE_0_WEIGHT;
use crate::sync::spinlock::Spinlock;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;

/// Default time slice for fair threads in nanoseconds (10 ms).
pub const BASE_SLICE_NS: u64 = 10_000_000;

/// Cached scheduling entity for the EEVDF run queue.
///
/// Caching scheduling parameters directly in the entity avoids acquiring
/// individual `Thread` spinlocks during `pick_next` candidate evaluation.
#[derive(Clone)]
pub struct EevdfEntity {
    pub tid: ThreadId,
    pub vruntime: u64,
    pub vdeadline: u64,
    pub weight: u32,
    pub slice_ns: u64,
    pub thread: Arc<Spinlock<Thread>>,
}

/// The Earliest Eligible Virtual Deadline First (EEVDF) Fair Scheduler.
pub struct EevdfScheduler {
    /// The run queue of ready fair threads indexed by `ThreadId`.
    run_queue: BTreeMap<ThreadId, EevdfEntity>,

    /// Monotonic system virtual time baseline.
    pub min_vruntime: u64,
}

impl EevdfScheduler {
    /// Creates a new, empty `EevdfScheduler`.
    pub const fn new() -> Self {
        Self {
            run_queue: BTreeMap::new(),
            min_vruntime: 0,
        }
    }

    /// Adds a thread to the fair run queue.
    ///
    /// Normalizes `vruntime` against `min_vruntime` and computes its virtual deadline.
    pub fn add_thread(&mut self, thread: Arc<Spinlock<Thread>>) {
        let mut t_lock = thread.lock();

        // Prevent waking threads from gaining unfair CPU time if they slept for long.
        if t_lock.vruntime < self.min_vruntime {
            t_lock.vruntime = self.min_vruntime;
        }

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

        // Calculate virtual deadline: d_i = v_i + (q_i * w_0) / w_i
        let vslice = (slice_ns * NICE_0_WEIGHT as u64) / weight as u64;
        t_lock.vdeadline = t_lock.vruntime.saturating_add(vslice);
        t_lock.state = ThreadState::Ready;

        let tid = t_lock.tid;
        let vruntime = t_lock.vruntime;
        let vdeadline = t_lock.vdeadline;
        drop(t_lock);

        let entity = EevdfEntity {
            tid,
            vruntime,
            vdeadline,
            weight,
            slice_ns,
            thread,
        };

        self.run_queue.insert(tid, entity);
    }

    /// Removes a thread from the fair run queue by its `ThreadId`.
    pub fn remove_thread(&mut self, tid: ThreadId) -> Option<Arc<Spinlock<Thread>>> {
        self.run_queue.remove(&tid).map(|e| e.thread)
    }

    /// Picks the next eligible fair thread with the earliest virtual deadline.
    ///
    /// Evaluation is performed directly against cached metadata without locking threads.
    pub fn pick_next(&mut self) -> Option<Arc<Spinlock<Thread>>> {
        if self.run_queue.is_empty() {
            return None;
        }

        // Step 1: Find min vruntime across all queued threads to advance system virtual time if needed.
        let mut min_rq_vruntime = u64::MAX;
        for entity in self.run_queue.values() {
            if entity.vruntime < min_rq_vruntime {
                min_rq_vruntime = entity.vruntime;
            }
        }

        if min_rq_vruntime != u64::MAX && self.min_vruntime < min_rq_vruntime {
            self.min_vruntime = min_rq_vruntime;
        }

        // Step 2: Select the eligible thread with the earliest virtual deadline.
        // Eligibility criterion: vruntime <= self.min_vruntime
        let mut best_tid: Option<ThreadId> = None;
        let mut best_deadline = u64::MAX;
        let mut best_vruntime = u64::MAX;

        for entity in self.run_queue.values() {
            let is_eligible = entity.vruntime <= self.min_vruntime;

            if is_eligible
                && (entity.vdeadline < best_deadline
                    || (entity.vdeadline == best_deadline && entity.vruntime < best_vruntime))
            {
                best_deadline = entity.vdeadline;
                best_vruntime = entity.vruntime;
                best_tid = Some(entity.tid);
            }
        }

        // Fallback: If no thread was strictly eligible due to discrete tick advancement,
        // select the one with the earliest deadline.
        if best_tid.is_none() {
            for entity in self.run_queue.values() {
                if entity.vdeadline < best_deadline
                    || (entity.vdeadline == best_deadline && entity.vruntime < best_vruntime)
                {
                    best_deadline = entity.vdeadline;
                    best_vruntime = entity.vruntime;
                    best_tid = Some(entity.tid);
                }
            }
        }

        let selected_tid = best_tid?;
        let selected_entity = self.run_queue.remove(&selected_tid)?;

        let mut t_lock = selected_entity.thread.lock();
        t_lock.state = ThreadState::Running;
        let selected_vruntime = t_lock.vruntime;
        drop(t_lock);

        // Advance system virtual time monotonically to selected thread's vruntime
        if selected_vruntime > self.min_vruntime {
            self.min_vruntime = selected_vruntime;
        }

        Some(selected_entity.thread)
    }

    /// Checks if the fair run queue is empty.
    pub fn is_empty(&self) -> bool {
        self.run_queue.is_empty()
    }
}
