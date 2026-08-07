//! Per-CPU scheduler and scheduling policy engine.
//!
//! [`PerCpuScheduler`] owns one [`CfsRunQueue`] and one [`RtRunQueue`] for a
//! single logical CPU. It implements the **scheduling policy**:
//!
//! > If any real-time thread is runnable, the RT scheduler picks next.
//! > Otherwise the CFS scheduler picks next.
//!
//! This mirrors Linux's classic RT-over-CFS hierarchy and ensures that
//! real-time threads always preempt normal ones.

use crate::sched::{
    cfs::CfsRunQueue,
    realtime::RtRunQueue,
    sched_thread::{SchedThread, ThreadId},
};

// ── Per-CPU scheduler ────────────────────────────────────────────────────────

/// A per-CPU scheduler that combines a CFS run queue with an RT run queue and
/// applies the correct scheduling policy at each scheduling decision.
pub struct PerCpuScheduler {
    /// Logical CPU identifier (e.g. LAPIC ID or zero-based index).
    pub cpu_id: u32,
    /// The CFS run queue for `Normal` threads.
    cfs: CfsRunQueue,
    /// The RT run queue for `Fifo` / `RoundRobin` threads.
    rt: RtRunQueue,
    /// The `SchedThread` descriptor that is currently occupying the CPU, if any.
    pub running: Option<SchedThread>,
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

    // ── Thread management ───────────────────────────────────────────────────────

    /// Add a thread to the appropriate run queue based on its scheduling policy.
    pub fn add_thread(&mut self, thread: SchedThread) {
        if thread.policy.is_realtime() {
            self.rt.enqueue(thread);
        } else {
            self.cfs.enqueue(thread);
        }
    }

    /// Remove a thread by `id` from whichever queue holds it.
    ///
    /// Also clears `running` if the thread is the currently executing one.
    pub fn remove_thread(&mut self, id: ThreadId) -> Option<SchedThread> {
        if self.running.as_ref().map(|t| t.id) == Some(id) {
            return self.running.take();
        }
        // Try RT first (more common to remove running RT threads).
        if let removed @ Some(_) = self.rt.remove(id) {
            return removed;
        }
        self.cfs.remove(id)
    }

    // ── Scheduling decision ────────────────────────────────────────────────────

    /// Select the next thread to run on this CPU.
    ///
    /// **Policy**:
    /// 1. If there are any runnable RT threads → dequeue from RT.
    /// 2. Otherwise → dequeue from CFS.
    ///
    /// The chosen thread is stored in `self.running` and its `id` is returned.
    /// Returns `None` if both queues are empty (idle CPU).
    pub fn schedule(&mut self) -> Option<ThreadId> {
        // Re-enqueue the currently running thread if it is still active/runnable
        if let Some(prev_thread) = self.running.take() {
            self.add_thread(prev_thread);
        }

        let next = if !self.rt.is_empty() {
            self.rt.dequeue_next()
        } else {
            self.cfs.dequeue_min()
        };

        match next {
            Some(thread) => {
                let id = thread.id;
                // The thread has been dequeued — it is now "running".
                self.running = Some(thread);
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
    /// running thread.
    ///
    /// * For **CFS** threads: increments `vruntime`.
    /// * For **RR** threads: decrements the remaining slice.
    ///
    /// Has no effect if no thread is currently running.
    pub fn thread_tick(&mut self, delta_ns: u64) {
        let Some(ref mut thread) = self.running else {
            return;
        };

        if thread.policy.is_realtime() {
            if thread.policy == crate::sched::sched_thread::SchedPolicy::RoundRobin {
                thread.remaining_slice = thread.remaining_slice.saturating_sub(delta_ns);
                if thread.remaining_slice == 0 {
                    thread.remaining_slice = thread.time_slice_ns;
                }
            }
        } else {
            // CFS threads: vruntime += delta_ns * NICE_0_WEIGHT / thread_weight
            let weight = thread.priority.max(1) as u64;
            thread.vruntime = thread.vruntime.saturating_add(
                delta_ns.saturating_mul(crate::sched::sched_thread::NICE_0_WEIGHT) / weight,
            );
        }
    }

    // ── Introspection ─────────────────────────────────────────────────────────

    /// Total number of runnable threads (RT + CFS).
    pub fn runnable_count(&self) -> usize {
        self.rt.len() + self.cfs.len()
    }

    /// The `ThreadId` of the currently executing thread, if any.
    pub fn running_thread(&self) -> Option<ThreadId> {
        self.running.as_ref().map(|t| t.id)
    }

    /// `true` if the RT run queue has any runnable threads.
    pub fn has_rt_tasks(&self) -> bool {
        !self.rt.is_empty()
    }

    /// `true` if the CFS run queue has any runnable threads.
    pub fn has_cfs_tasks(&self) -> bool {
        !self.cfs.is_empty()
    }
}
