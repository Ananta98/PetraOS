//! Per-CPU runqueue set (`PerCpuClassRqSet`).

use alloc::sync::Arc;
use core::sync::atomic::Ordering;

use ostd::task::Task;
use ostd::task::scheduler::info::CommonSchedInfo;
use ostd::task::scheduler::{LocalRunQueue, UpdateFlags};

use super::fair::FairRunQueue;
use super::policy::SchedClassPolicy;
use super::real_time::RtRunQueue;
use super::{SchedClass, TaskData};

/// Represents the run queue set for a single CPU core.
///
/// Holds the active task currently executing on this CPU core along with
/// the per-class runqueues (`real_time`, `fair`) and the CPU's idle thread.
#[derive(Debug)]
pub struct PerCpuClassRqSet {
    pub current: Option<Arc<Task>>,
    pub real_time: RtRunQueue,
    pub fair: FairRunQueue,
    /// The per-CPU idle thread. Always present once the CPU is brought up;
    /// it is picked only when no real-time or fair task is runnable and never
    /// blocks, so the runqueue always has a task to fall back to.
    pub idle: Option<Arc<Task>>,
    pub vtime: u64,
    pub nr_runnable: usize,
}

impl PerCpuClassRqSet {
    /// Create a new empty `PerCpuClassRqSet`.
    pub const fn new() -> Self {
        Self {
            current: None,
            real_time: RtRunQueue::new(),
            fair: FairRunQueue::new(),
            idle: None,
            vtime: 0,
            nr_runnable: 0,
        }
    }

    /// Minimum virtual runtime of tasks in this per-CPU run queue set.
    pub fn min_vruntime(&self) -> u64 {
        let mut min_val = if let Some(curr) = &self.current {
            let (class, vruntime) = crate::scheduler::get_sched_data(curr);
            match class {
                SchedClass::RealTime { .. } | SchedClass::Idle => 0,
                SchedClass::Fair { .. } => vruntime,
            }
        } else {
            0
        };
        if let Some(fair_min) = self.fair.min_vruntime() {
            if min_val == 0 {
                min_val = fair_min;
            } else {
                min_val = min_val.min(fair_min);
            }
        }
        min_val
    }

    fn total_fair_weight(&self) -> u64 {
        let mut total = self.fair.total_weight();
        if let Some(curr) = &self.current {
            let (class, _) = crate::scheduler::get_sched_data(curr);
            if let SchedClass::Fair { nice } = class {
                total += nice.to_weight();
            }
        }
        total
    }

    /// Enqueue a task into the appropriate scheduling class runqueue.
    pub fn enqueue_task(&mut self, task: Arc<Task>) {
        let (class, vruntime) = crate::scheduler::get_sched_data(&task);
        match class {
            SchedClass::RealTime { priority } => {
                let mut effective_priority = priority;
                if let Some(data) = TaskData::from_task(&task) {
                    let ema = data.ema.load(Ordering::Relaxed);
                    let decay = (ema * 10 / 2_000_000) as u32; // Drop up to 10 prio levels based on CPU usage
                    effective_priority = priority.saturating_sub(decay);
                }
                self.real_time.enqueue_prio(task, effective_priority);
            }
            SchedClass::Fair { .. } => {
                self.fair.enqueue_fair(task, vruntime, self.vtime);
            }
            SchedClass::Idle => {
                debug_assert!(self.idle.is_none(), "only one idle task per CPU");
                self.idle = Some(task);
                // The idle thread is not counted as a runnable task.
                return;
            }
        }
        self.nr_runnable += 1;
        self.vtime = self.vtime.max(self.min_vruntime());
    }

    /// Determine whether a queued task should preempt the currently executing task.
    pub fn should_preempt_current(&self) -> bool {
        // OSTD's `LocalRunQueue` contract requires that when the runqueue is
        // non-empty but no task is currently executing, we must report that a
        // preemption is needed — "anything is better than nothing".  This is
        // essential for scheduling the very first task: the boot task that
        // runs `kernel_main` executes outside the runqueue, so `current` is
        // `None` until the first context switch. The per-CPU idle thread also
        // qualifies as a valid first target.
        let Some(curr) = &self.current else {
            return self.nr_runnable > 0 || self.idle.is_some();
        };

        let (curr_class, curr_vruntime) = crate::scheduler::get_sched_data(curr);

        match curr_class {
            SchedClass::RealTime {
                priority: curr_priority,
            } => {
                if let Some(highest_priority) = self.real_time.highest_priority() {
                    if highest_priority > curr_priority {
                        return true;
                    }
                }
                false
            }
            SchedClass::Fair { .. } => {
                if !self.real_time.is_empty() {
                    return true;
                }

                if self.fair.is_empty() {
                    return false;
                }

                let curr_deadline = TaskData::from_task(curr)
                    .map(|d| d.deadline(curr_vruntime))
                    .unwrap_or(curr_vruntime + 1024_000);
                for (&vruntime, queue) in self.fair.tasks.range(..=self.vtime) {
                    for task in queue {
                        let deadline = TaskData::from_task(task)
                            .map(|d| d.deadline(vruntime))
                            .unwrap_or(vruntime + 1024_000);
                        if deadline + 1000 < curr_deadline {
                            return true;
                        }
                    }
                }
                false
            }
            SchedClass::Idle => {
                // The idle thread must yield to any runnable real task.
                self.nr_runnable > 0
            }
        }
    }
}

impl LocalRunQueue<Task> for PerCpuClassRqSet {
    fn current(&self) -> Option<&Arc<Task>> {
        self.current.as_ref()
    }

    fn update_current(&mut self, flags: UpdateFlags) -> bool {
        if let Some(curr) = &self.current {
            let (class, vruntime) = crate::scheduler::get_sched_data(curr);
            match class {
                SchedClass::RealTime { .. } => {
                    if flags == UpdateFlags::Tick {
                        if let Some(data) = TaskData::from_task(curr) {
                            let ema = data.ema.load(Ordering::Relaxed);
                            let rt_budget: u64 = 2_000_000;
                            let delta_ns: u64 = 1_000_000;
                            if ema < rt_budget {
                                let ema_delta =
                                    (rt_budget - ema) * delta_ns * 16 / (rt_budget * 256);
                                data.ema.store(ema + ema_delta, Ordering::Relaxed);
                            }
                        }
                    }
                }
                SchedClass::Fair { nice } => {
                    if flags == UpdateFlags::Tick {
                        let weight = nice.to_weight();
                        let delta = 1000;
                        let vruntime_delta = delta * 1024 / weight.max(1);

                        if let Some(data) = TaskData::from_task(curr) {
                            data.set_vruntime(vruntime + vruntime_delta);
                            let ema = data.ema.load(Ordering::Relaxed);
                            let budget_max: u64 = 2_000_000;
                            let delta_ns: u64 = 1_000_000;
                            if ema < budget_max {
                                let ema_delta =
                                    (budget_max - ema) * delta_ns * 16 / (budget_max * 256);
                                data.ema.store(ema + ema_delta, Ordering::Relaxed);
                            }
                        }

                        let total_w = self.total_fair_weight().max(1);
                        let vtime_delta = delta * 1024 / total_w;
                        self.vtime += vtime_delta;
                    }
                }
                SchedClass::Idle => {
                    // The idle thread never accumulates scheduling statistics.
                }
            }
        }

        self.vtime = self.vtime.max(self.min_vruntime());

        match flags {
            UpdateFlags::Tick => self.should_preempt_current(),
            UpdateFlags::Wait | UpdateFlags::Yield | UpdateFlags::Exit => {
                // The per-CPU idle thread is always a valid fallback, so a
                // blocking or exiting task never forces a busy-wait.
                self.nr_runnable > 0 || self.idle.is_some()
            }
        }
    }

    fn try_pick_next(&mut self) -> Option<&Arc<Task>> {
        let next_task = if !self.real_time.is_empty() {
            let task = SchedClassPolicy::pick_next(&mut self.real_time, self.vtime).unwrap();
            self.nr_runnable -= 1;
            task
        } else if !self.fair.is_empty() {
            let task = SchedClassPolicy::pick_next(&mut self.fair, self.vtime).unwrap();
            self.nr_runnable -= 1;
            task
        } else if let Some(idle) = self.idle.take() {
            // Take the idle task out of self.idle so the slot is vacant
            // while the idle thread is executing. It will be restored below
            // if / when it is displaced from `current`.
            idle
        } else {
            return None;
        };

        if let Some(prev_task) = self.current.replace(next_task) {
            let (prev_class, _) = crate::scheduler::get_sched_data(&prev_task);
            if matches!(prev_class, SchedClass::Idle) {
                // The idle task is never counted in nr_runnable; put it back
                // into its dedicated slot instead of going through enqueue_task.
                self.idle = Some(prev_task);
            } else {
                self.enqueue_task(prev_task);
            }
        }

        self.vtime = self.vtime.max(self.min_vruntime());
        self.current.as_ref()
    }

    fn dequeue_current(&mut self) -> Option<Arc<Task>> {
        let res = self.current.take().inspect(|task| task.cpu().set_to_none());
        if let Some(task) = &res {
            if let Some(data) = task.data().downcast_ref::<TaskData>() {
                data.last_dequeue_vtime.store(self.vtime, Ordering::Relaxed);
            }
        }
        self.vtime = self.vtime.max(self.min_vruntime());
        res
    }
}
