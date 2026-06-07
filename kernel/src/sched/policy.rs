//! Per-CPU scheduler and scheduling policy engine.
//!
//! [`PerCpuScheduler`] owns one [`CfsRunQueue`] and one [`RtRunQueue`] for a
//! single logical CPU. It implements the **scheduling policy**:
//!
//! > If any real-time task is runnable, the RT scheduler picks next.
//! > Otherwise the CFS scheduler picks next.
//!
//! This mirrors Linux's classic RT-over-CFS hierarchy and ensures that
//! real-time tasks always preempt normal ones.

use crate::sched::{
    cfs::CfsRunQueue,
    rt::RtRunQueue,
    task::{Task, TaskId},
};

// ── Per-CPU scheduler ────────────────────────────────────────────────────────

/// A per-CPU scheduler that combines a CFS run queue with an RT run queue and
/// applies the correct scheduling policy at each scheduling decision.
pub struct PerCpuScheduler {
    /// Logical CPU identifier (e.g. LAPIC ID or zero-based index).
    pub cpu_id: u32,
    /// The CFS run queue for `Normal` tasks.
    cfs: CfsRunQueue,
    /// The RT run queue for `Fifo` / `RoundRobin` tasks.
    rt: RtRunQueue,
    /// The `Task` descriptor that is currently occupying the CPU, if any.
    pub running: Option<Task>,
}

impl PerCpuScheduler {
    /// Create a new per-CPU scheduler for `cpu_id`.
    pub fn new(cpu_id: u32) -> Self {
        Self {
            cpu_id,
            cfs: CfsRunQueue::new(),
            rt: RtRunQueue::new(),
            running: None,
        }
    }

    // ── Task management ───────────────────────────────────────────────────────

    /// Add a task to the appropriate run queue based on its scheduling policy.
    pub fn add_task(&mut self, task: Task) {
        if task.policy.is_realtime() {
            self.rt.enqueue(task);
        } else {
            self.cfs.enqueue(task);
        }
    }

    /// Remove a task by `id` from whichever queue holds it.
    ///
    /// Also clears `running` if the task is the currently executing one.
    pub fn remove_task(&mut self, id: TaskId) -> Option<Task> {
        if self.running.as_ref().map(|t| t.id) == Some(id) {
            return self.running.take();
        }
        // Try RT first (more common to remove running RT tasks).
        if let removed @ Some(_) = self.rt.remove(id) {
            return removed;
        }
        self.cfs.remove(id)
    }

    // ── Scheduling decision ────────────────────────────────────────────────────

    /// Select the next task to run on this CPU.
    ///
    /// **Policy**:
    /// 1. If there are any runnable RT tasks → dequeue from RT.
    /// 2. Otherwise → dequeue from CFS.
    ///
    /// The chosen task is stored in `self.running` and its `id` is returned.
    /// Returns `None` if both queues are empty (idle CPU).
    pub fn schedule(&mut self) -> Option<TaskId> {
        // Re-enqueue the currently running task if it is still active/runnable
        if let Some(prev_task) = self.running.take() {
            self.add_task(prev_task);
        }

        let next = if !self.rt.is_empty() {
            self.rt.dequeue_next()
        } else {
            self.cfs.dequeue_min()
        };

        match next {
            Some(task) => {
                let id = task.id;
                // The task has been dequeued — it is now "running".
                self.running = Some(task);
                Some(id)
            }
            None => {
                self.running = None;
                None
            }
        }
    }

    // ── Timer tick ────────────────────────────────────────────────────────────

    /// Advance scheduling state by `delta_ns` nanoseconds for the currently
    /// running task.
    ///
    /// * For **CFS** tasks: increments `vruntime`.
    /// * For **RR** tasks: decrements the remaining slice.
    ///
    /// Has no effect if no task is currently running.
    pub fn task_tick(&mut self, delta_ns: u64) {
        let Some(ref mut task) = self.running else {
            return;
        };

        if task.policy.is_realtime() {
            if task.policy == crate::sched::task::SchedPolicy::RoundRobin {
                task.remaining_slice = task.remaining_slice.saturating_sub(delta_ns);
                if task.remaining_slice == 0 {
                    task.remaining_slice = task.time_slice_ns;
                }
            }
        } else {
            // CFS tasks: vruntime += delta_ns * NICE_0_WEIGHT / task_weight
            let weight = task.priority.max(1) as u64;
            task.vruntime = task
                .vruntime
                .saturating_add(delta_ns.saturating_mul(crate::sched::task::NICE_0_WEIGHT) / weight);
        }
    }

    // ── Introspection ─────────────────────────────────────────────────────────

    /// Total number of runnable tasks (RT + CFS).
    pub fn runnable_count(&self) -> usize {
        self.rt.len() + self.cfs.len()
    }

    /// The `TaskId` of the currently executing task, if any.
    pub fn running_task(&self) -> Option<TaskId> {
        self.running.as_ref().map(|t| t.id)
    }

    /// `true` if the RT run queue has any runnable tasks.
    pub fn has_rt_tasks(&self) -> bool {
        !self.rt.is_empty()
    }

    /// `true` if the CFS run queue has any runnable tasks.
    pub fn has_cfs_tasks(&self) -> bool {
        !self.cfs.is_empty()
    }
}
