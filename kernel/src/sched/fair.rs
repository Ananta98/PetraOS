//! Earliest Eligible Virtual Deadline First (EEVDF) Scheduler.
//!
//! Implements proportional-share fair scheduling based on:
//! 1. Virtual runtime (`vruntime`): accumulated CPU execution time scaled by thread weight.
//! 2. System virtual time (`min_vruntime`): monotonic virtual time baseline of the system.
//! 3. Eligibility: a thread $i$ is eligible to run when $v_i \le \text{min\_vruntime}$.
//! 4. Virtual deadline (`vdeadline`): virtual time by which the allocated time slice
//!    should complete, computed as $d_i = v_i + \frac{q_i \cdot w_0}{w_i}$.

use crate::proc::thread::{Thread, ThreadId, ThreadState};
use crate::sched::nice::NICE_0_WEIGHT;
use crate::sync::Mutex;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;

/// Default time slice for fair threads in nanoseconds (10 ms).
pub const BASE_SLICE_NS: u64 = 10_000_000;

/// Cached scheduling entity for the EEVDF run queue.
///
/// Caching scheduling parameters directly in the entity avoids acquiring
/// the thread Mutex during `pick_next` candidate evaluation.
#[derive(Clone)]
pub struct EevdfEntity {
    pub tid: ThreadId,
    pub vruntime: u64,
    pub vdeadline: u64,
    pub weight: u32,
    pub slice_ns: u64,
    pub thread: Arc<Mutex<Thread>>,
}

/// The Earliest Eligible Virtual Deadline First (EEVDF) Fair Scheduler.
pub struct EevdfScheduler {
    /// Ordered run queue keyed by `(vdeadline, ThreadId)` for O(log N) pick_next.
    timeline: BTreeMap<(u64, ThreadId), EevdfEntity>,

    /// Secondary index mapping `ThreadId` → `vdeadline` for O(log N) dequeue.
    by_tid: BTreeMap<ThreadId, u64>,

    /// Monotonic system virtual time baseline.
    pub min_vruntime: u64,
}

impl EevdfScheduler {
    /// Creates a new, empty `EevdfScheduler`.
    pub const fn new() -> Self {
        Self {
            timeline: BTreeMap::new(),
            by_tid: BTreeMap::new(),
            min_vruntime: 0,
        }
    }

    /// Adds a thread to the fair run queue.
    ///
    /// Normalizes `vruntime` against `min_vruntime` and computes its virtual deadline.
    pub fn add_thread(&mut self, thread: Arc<Mutex<Thread>>) {
        let mut t = thread.lock();

        // Prevent waking threads from gaining unfair CPU time after long sleeps.
        if t.vruntime < self.min_vruntime {
            t.vruntime = self.min_vruntime;
        }

        let weight = if t.weight > 0 { t.weight } else { NICE_0_WEIGHT };
        let slice_ns = if t.slice_ns > 0 { t.slice_ns } else { BASE_SLICE_NS };

        // Virtual deadline: d_i = v_i + (q_i * w_0) / w_i
        let vslice = (slice_ns * NICE_0_WEIGHT as u64) / weight as u64;
        t.vdeadline = t.vruntime.saturating_add(vslice);
        t.state = ThreadState::Ready;

        let (tid, vruntime, vdeadline) = (t.tid, t.vruntime, t.vdeadline);
        drop(t);

        let entity = EevdfEntity { tid, vruntime, vdeadline, weight, slice_ns, thread };
        self.timeline.insert((vdeadline, tid), entity);
        self.by_tid.insert(tid, vdeadline);
    }

    /// Removes a thread from the fair run queue by its `ThreadId`.
    pub fn remove_thread(&mut self, tid: ThreadId) -> Option<Arc<Mutex<Thread>>> {
        let vdeadline = self.by_tid.remove(&tid)?;
        self.timeline.remove(&(vdeadline, tid)).map(|e| e.thread)
    }

    /// Picks the next eligible fair thread with the earliest virtual deadline.
    pub fn pick_next(&mut self) -> Option<Arc<Mutex<Thread>>> {
        if self.timeline.is_empty() {
            return None;
        }

        // Advance min_vruntime to the minimum vruntime across all queued threads.
        let min_rq_vr = self.timeline.values().map(|e| e.vruntime).min().unwrap_or(self.min_vruntime);
        if min_rq_vr > self.min_vruntime {
            self.min_vruntime = min_rq_vr;
        }

        // Find the eligible thread (vruntime <= min_vruntime) with the earliest virtual deadline.
        let mut chosen_key = None;
        for (&(vdeadline, tid), entity) in self.timeline.iter() {
            if entity.vruntime <= self.min_vruntime {
                chosen_key = Some((vdeadline, tid));
                break;
            }
        }

        // Fall back to the earliest-deadline thread if none is strictly eligible.
        let key = chosen_key.or_else(|| self.timeline.keys().next().copied())?;
        let entity = self.timeline.remove(&key)?;
        self.by_tid.remove(&entity.tid);

        if entity.vruntime > self.min_vruntime {
            self.min_vruntime = entity.vruntime;
        }

        Some(entity.thread)
    }

    /// Enqueues a runnable thread into the fair run queue.
    pub fn enqueue(&mut self, thread: Arc<Mutex<Thread>>) {
        self.add_thread(thread);
    }

    /// Removes a thread from the run queue by its `ThreadId`.
    pub fn dequeue(&mut self, tid: ThreadId) -> Option<Arc<Mutex<Thread>>> {
        self.remove_thread(tid)
    }

    /// Updates virtual runtime accounting and returns `true` if preemption should trigger.
    pub fn update_current(&mut self, thread: &mut Thread, delta_ns: u64) -> bool {
        let weight = if thread.weight > 0 { thread.weight } else { NICE_0_WEIGHT };
        let vruntime_delta = (delta_ns * NICE_0_WEIGHT as u64) / weight as u64;
        thread.vruntime = thread.vruntime.saturating_add(vruntime_delta);
        thread.vruntime >= thread.vdeadline
    }

    /// Returns the number of runnable fair entities.
    #[inline]
    pub fn len(&self) -> usize {
        self.timeline.len()
    }

    /// Checks whether the fair run queue is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.timeline.is_empty()
    }
}
