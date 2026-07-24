//! Trait definitions for scheduling policies (`SchedClassPolicy`).

use alloc::sync::Arc;
use core::fmt;
use ostd::task::Task;

/// The unified scheduling class trait (`SchedClassPolicy`) implemented by all scheduling policies.
///
/// Encapsulates policy-specific task management, enqueueing, task selection,
/// preemption checking, and runqueue length querying.
pub trait SchedClassPolicy: Send + fmt::Debug {
    /// Enqueue a runnable task into this scheduling class's run queue.
    fn enqueue(&mut self, task: Arc<Task>, vtime: u64);

    /// Pick the next task to run from this scheduling class.
    fn pick_next(&mut self, vtime: u64) -> Option<Arc<Task>>;

    /// Check if a newly enqueued/runnable task should preempt the currently running task.
    fn check_preempt_curr(&self, curr: &Task, newcomer: &Task, vtime: u64) -> bool;

    /// Return the total number of tasks in this scheduling class's run queue.
    fn len(&self) -> usize;

    /// Check if this scheduling class's run queue is empty.
    ///
    /// Default implementation checks if `len() == 0`.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
