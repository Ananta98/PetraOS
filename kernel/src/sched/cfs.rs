//! Completely Fair Scheduler (CFS) run queue.
//!
//! Uses an [`alloc::collections::BTreeMap`] keyed on `(vruntime, TaskId)` to
//! maintain tasks in virtual-runtime order with O(log n) enqueue and
//! O(log n) dequeue-min — functionally equivalent to Linux's per-CPU CFS
//! red-black tree.
//!
//! # Virtual runtime formula
//!
//! ```text
//! vruntime += delta_real_ns * NICE_0_WEIGHT / task_weight
//! ```
//!
//! Tasks with a higher CFS weight (lower nice value / higher priority) advance
//! their vruntime more slowly, giving them a larger share of real CPU time.

extern crate alloc;

use alloc::collections::BTreeMap;

use crate::sched::task::{Task, TaskId, NICE_0_WEIGHT};

// ── CFS run queue ─────────────────────────────────────────────────────────────

/// A CFS run queue for a single CPU.
///
/// Tasks are kept in a `BTreeMap` keyed by `(vruntime, TaskId)`. The composite
/// key guarantees strict total order even when two tasks share the same
/// `vruntime`, which avoids accidental collisions (BTreeMap does not allow
/// duplicate keys).
pub struct CfsRunQueue {
    /// Ordered map: (vruntime, id) → Task.
    tree: BTreeMap<(u64, TaskId), Task>,
    /// The minimum vruntime ever dequeued from this run queue.
    ///
    /// New tasks are placed at `max(0, min_vruntime)` so they do not
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

    /// Returns the number of runnable tasks.
    #[inline]
    pub fn len(&self) -> usize {
        self.tree.len()
    }

    /// Returns `true` if no tasks are runnable.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    // ── Enqueueing ────────────────────────────────────────────────────────────

    /// Insert a task into the run queue.
    ///
    /// If the task's `vruntime` is below the current [`min_vruntime`], it is
    /// lifted to `min_vruntime` so it does not jump ahead of already-running
    /// tasks.
    pub fn enqueue(&mut self, mut task: Task) {
        // Lift new/returning tasks to the queue floor to ensure fairness.
        if task.vruntime < self.min_vruntime {
            task.vruntime = self.min_vruntime;
        }
        self.tree.insert((task.vruntime, task.id), task);
    }

    // ── Dequeueing ────────────────────────────────────────────────────────────

    /// Remove and return the task with the smallest `vruntime` (the next task
    /// to run), or `None` if the queue is empty.
    pub fn dequeue_min(&mut self) -> Option<Task> {
        let key = *self.tree.keys().next()?;
        let task = self.tree.remove(&key)?;
        // Advance the floor so future tasks cannot exploit the old minimum.
        self.min_vruntime = self.min_vruntime.max(task.vruntime);
        Some(task)
    }

    // ── Peeking ───────────────────────────────────────────────────────────────

    /// Return a reference to the task with the smallest `vruntime` without
    /// removing it, or `None` if the queue is empty.
    pub fn pick_next(&self) -> Option<&Task> {
        self.tree.values().next()
    }

    // ── Virtual runtime updates ───────────────────────────────────────────────

    /// Update the `vruntime` of a task identified by `id` and re-insert it at
    /// its new position in the tree.
    ///
    /// * `delta_ns` — real elapsed nanoseconds since the task was last
    ///   scheduled.
    /// * Returns `true` if the task was found and updated.
    pub fn update_vruntime(&mut self, id: TaskId, delta_ns: u64) -> bool {
        // Find the current key for this task (we must search by TaskId).
        let key = self
            .tree
            .iter()
            .find(|((_vr, tid), _)| *tid == id)
            .map(|(k, _)| *k);

        let Some(old_key) = key else {
            return false;
        };

        let Some(mut task) = self.tree.remove(&old_key) else {
            return false;
        };

        // vruntime += delta_ns * NICE_0_WEIGHT / task_weight
        let weight = task.priority.max(1) as u64;
        task.vruntime = task
            .vruntime
            .saturating_add(delta_ns.saturating_mul(NICE_0_WEIGHT) / weight);

        self.tree.insert((task.vruntime, task.id), task);
        true
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// The current minimum vruntime floor.
    #[inline]
    pub fn min_vruntime(&self) -> u64 {
        self.min_vruntime
    }

    /// Remove a task by `id` regardless of its position.
    ///
    /// Returns the task if found.
    pub fn remove(&mut self, id: TaskId) -> Option<Task> {
        let key = self
            .tree
            .iter()
            .find(|((_vr, tid), _)| *tid == id)
            .map(|(k, _)| *k)?;
        self.tree.remove(&key)
    }
}
