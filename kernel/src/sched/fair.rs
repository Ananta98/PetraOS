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
use crate::sync::Mutex;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;

/// Default time slice for fair threads in nanoseconds (10 ms).
pub const BASE_SLICE_NS: u64 = 10_000_000;

/// Cached scheduling entity for the EEVDF run queue.
///
/// Caching scheduling parameters directly in the entity avoids acquiring
/// individual `Thread` mutexes during `pick_next` candidate evaluation.
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
    /// Ordered run queue keyed by `(vdeadline, ThreadId)` for O(1)/O(log N) pick_next.
    timeline: BTreeMap<(u64, ThreadId), EevdfEntity>,

    /// Secondary index mapping `ThreadId` to `vdeadline` for O(log N) dequeue.
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

        // Advance min_vruntime if all queued threads have advanced past it
        let min_rq_vruntime = self.timeline.values().map(|e| e.vruntime).min().unwrap_or(self.min_vruntime);
        if min_rq_vruntime > self.min_vruntime {
            self.min_vruntime = min_rq_vruntime;
        }

        // Find the eligible thread (vruntime <= min_vruntime) with earliest virtual deadline.
        // Since timeline is ordered by vdeadline, the first eligible candidate has the earliest deadline.
        let mut chosen_key = None;
        for (&(vdeadline, tid), entity) in self.timeline.iter() {
            if entity.vruntime <= self.min_vruntime {
                chosen_key = Some((vdeadline, tid));
                break;
            }
        }

        // If no strictly eligible entity is found, pick the one with earliest deadline
        let key = chosen_key.or_else(|| self.timeline.keys().next().copied())?;
        let entity = self.timeline.remove(&key)?;
        self.by_tid.remove(&entity.tid);

        // Advance min_vruntime monotonically
        if entity.vruntime > self.min_vruntime {
            self.min_vruntime = entity.vruntime;
        }

        Some(entity.thread)
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

/// Fair Scheduling Class Run Queue wrapper.
pub struct FairClassRq {
    pub scheduler: EevdfScheduler,
}

impl FairClassRq {
    /// Creates a new `FairClassRq`.
    pub const fn new() -> Self {
        Self {
            scheduler: EevdfScheduler::new(),
        }
    }

    /// Returns the human-readable name of this scheduling class.
    pub fn name(&self) -> &'static str {
        "Fair"
    }

    /// Enqueues a runnable thread into this scheduling class run queue.
    pub fn enqueue(&mut self, thread: Arc<Mutex<Thread>>) {
        self.scheduler.add_thread(thread);
    }

    /// Removes a thread from the run queue by its `ThreadId`.
    pub fn dequeue(&mut self, tid: ThreadId) -> Option<Arc<Mutex<Thread>>> {
        self.scheduler.remove_thread(tid)
    }

    /// Picks the next thread to execute according to this class's policy.
    pub fn pick_next(&mut self) -> Option<Arc<Mutex<Thread>>> {
        self.scheduler.pick_next()
    }

    /// Returns the number of runnable threads queued in this class.
    pub fn len(&self) -> usize {
        self.scheduler.len()
    }

    /// Checks if this class run queue is empty.
    pub fn is_empty(&self) -> bool {
        self.scheduler.is_empty()
    }

    /// Updates virtual runtime and returns true if preemption should be triggered.
    pub fn update_current(&mut self, thread: &mut Thread, delta_ns: u64) -> bool {
        let weight = if thread.weight > 0 {
            thread.weight
        } else {
            NICE_0_WEIGHT
        };
        let vruntime_delta = (delta_ns * NICE_0_WEIGHT as u64) / weight as u64;
        thread.vruntime = thread.vruntime.saturating_add(vruntime_delta);

        // Preempt when thread's accumulated virtual runtime reaches or exceeds its virtual deadline
        thread.vruntime >= thread.vdeadline
    }
}

impl Default for FairClassRq {
    fn default() -> Self {
        Self::new()
    }
}
