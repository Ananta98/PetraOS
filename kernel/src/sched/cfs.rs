//! Completely Fair Scheduler (CFS) run queue.
//!
//! Uses an [`alloc::collections::BTreeMap`] keyed on `(vruntime, ThreadId)` to
//! maintain threads in virtual-runtime order with O(log n) enqueue and
//! O(log n) dequeue-min — functionally equivalent to Linux's per-CPU CFS
//! red-black tree.
//!
//! # Virtual runtime formula
//!
//! ```text
//! vruntime += delta_real_ns * NICE_0_WEIGHT / thread_weight
//! ```
//!
//! Threads with a higher CFS weight (lower nice value / higher priority) advance
//! their vruntime more slowly, giving them a larger share of real CPU time.

extern crate alloc;

use alloc::collections::BTreeMap;

use crate::sched::sched_thread::{SchedThread, ThreadId, NICE_0_WEIGHT};

// ── CFS run queue ─────────────────────────────────────────────────────────────

/// A CFS run queue for a single CPU.
///
/// Threads are kept in a `BTreeMap` keyed by `(vruntime, ThreadId)`. The composite
/// key guarantees strict total order even when two threads share the same
/// `vruntime`, which avoids accidental collisions (BTreeMap does not allow
/// duplicate keys).
pub struct CfsRunQueue {
    /// Ordered map: (vruntime, id) → SchedThread.
    tree: BTreeMap<(u64, ThreadId), SchedThread>,
    /// The minimum vruntime ever dequeued from this run queue.
    ///
    /// New threads are placed at `max(0, min_vruntime)` so they do not
    /// immediately monopolise the CPU after entering the run queue.
    min_vruntime: u64,
}

impl CfsRunQueue {
    /// Create an empty CFS run queue.
    pub const fn new() -> Self {
        Self {
            tree: BTreeMap::new(),
            min_vruntime: 0,
        }
    }

    // ── Capacity ─────────────────────────────────────────────────────────────

    /// Returns the number of runnable threads.
    #[inline]
    pub fn len(&self) -> usize {
        self.tree.len()
    }

    /// Returns `true` if no threads are runnable.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    // ── Enqueueing ────────────────────────────────────────────────────────────

    /// Insert a thread into the run queue.
    ///
    /// If the thread's `vruntime` is below the current [`min_vruntime`], it is
    /// lifted to `min_vruntime` so it does not jump ahead of already-running
    /// threads.
    pub fn enqueue(&mut self, mut thread: SchedThread) {
        // Lift new/returning threads to the queue floor to ensure fairness.
        if thread.vruntime < self.min_vruntime {
            thread.vruntime = self.min_vruntime;
        }
        self.tree.insert((thread.vruntime, thread.id), thread);
    }

    // ── Dequeueing ────────────────────────────────────────────────────────────

    /// Remove and return the thread with the smallest `vruntime` (the next thread
    /// to run), or `None` if the queue is empty.
    pub fn dequeue_min(&mut self) -> Option<SchedThread> {
        let key = *self.tree.keys().next()?;
        let thread = self.tree.remove(&key)?;
        // Advance the floor so future threads cannot exploit the old minimum.
        self.min_vruntime = self.min_vruntime.max(thread.vruntime);
        Some(thread)
    }

    // ── Peeking ───────────────────────────────────────────────────────────────

    /// Return a reference to the thread with the smallest `vruntime` without
    /// removing it, or `None` if the queue is empty.
    pub fn pick_next(&self) -> Option<&SchedThread> {
        self.tree.values().next()
    }

    // ── Virtual runtime updates ───────────────────────────────────────────────

    /// Update the `vruntime` of a thread identified by `id` and re-insert it at
    /// its new position in the tree.
    ///
    /// * `delta_ns` — real elapsed nanoseconds since the thread was last
    ///   scheduled.
    /// * Returns `true` if the thread was found and updated.
    pub fn update_vruntime(&mut self, id: ThreadId, delta_ns: u64) -> bool {
        // Find the current key for this thread (we must search by ThreadId).
        let key = self
            .tree
            .iter()
            .find(|((_vr, tid), _)| *tid == id)
            .map(|(k, _)| *k);

        let Some(old_key) = key else {
            return false;
        };

        let Some(mut thread) = self.tree.remove(&old_key) else {
            return false;
        };

        // vruntime += delta_ns * NICE_0_WEIGHT / thread_weight
        let weight = thread.priority.max(1) as u64;
        thread.vruntime = thread
            .vruntime
            .saturating_add(delta_ns.saturating_mul(NICE_0_WEIGHT) / weight);

        self.tree.insert((thread.vruntime, thread.id), thread);
        true
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// The current minimum vruntime floor.
    #[inline]
    pub fn min_vruntime(&self) -> u64 {
        self.min_vruntime
    }

    /// Remove a thread by `id` regardless of its position.
    ///
    /// Returns the thread if found.
    pub fn remove(&mut self, id: ThreadId) -> Option<SchedThread> {
        let key = self
            .tree
            .iter()
            .find(|((_vr, tid), _)| *tid == id)
            .map(|(k, _)| *k)?;
        self.tree.remove(&key)
    }
}
