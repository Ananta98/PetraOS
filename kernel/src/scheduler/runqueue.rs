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
/// the per-class runqueues (`real_time`, `fair`).
#[derive(Debug)]
pub struct PerCpuClassRqSet {
    pub current: Option<Arc<Task>>,
    pub real_time: RtRunQueue,
    pub fair: FairRunQueue,
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
            vtime: 0,
            nr_runnable: 0,
        }
    }

    /// Minimum virtual runtime of tasks in this per-CPU run queue set.
    pub fn min_vruntime(&self) -> u64 {
        let mut min_val = if let Some(curr) = &self.current {
            let (class, vruntime) = TaskData::sched_data(curr);
            match class {
                SchedClass::RealTime { .. } => 0,
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
            let (class, _) = TaskData::sched_data(curr);
            if let SchedClass::Fair { nice } = class {
                total += nice.to_weight();
            }
        }
        total
    }

    /// Enqueue a task into the appropriate scheduling class runqueue.
    pub fn enqueue_task(&mut self, task: Arc<Task>) {
        let (class, vruntime) = TaskData::sched_data(&task);
        match class {
            SchedClass::RealTime { priority } => {
                let mut effective_priority = priority;
                if let Some(data) = task.data().downcast_ref::<TaskData>() {
                    let ema = data.ema.load(Ordering::Relaxed);
                    let decay = (ema * 10 / 2_000_000) as u32; // Drop up to 10 prio levels based on CPU usage
                    effective_priority = priority.saturating_sub(decay);
                }
                self.real_time.enqueue_prio(task, effective_priority);
            }
            SchedClass::Fair { .. } => {
                self.fair.enqueue_fair(task, vruntime, self.vtime);
            }
        }
        self.nr_runnable += 1;
        self.vtime = self.vtime.max(self.min_vruntime());
    }

    /// Determine whether a queued task should preempt the currently executing task.
    pub fn should_preempt_current(&self) -> bool {
        let Some(curr) = &self.current else {
            return false;
        };

        let (curr_class, curr_vruntime) = TaskData::sched_data(curr);

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

                let curr_deadline = TaskData::deadline(curr_vruntime, curr_class, Some(&**curr));
                for (&vruntime, queue) in self.fair.tasks.range(..=self.vtime) {
                    for task in queue {
                        let (class, _) = TaskData::sched_data(task);
                        let deadline = TaskData::deadline(vruntime, class, Some(&**task));
                        if deadline + 1000 < curr_deadline {
                            return true;
                        }
                    }
                }
                false
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
            let (class, vruntime) = TaskData::sched_data(curr);
            match class {
                SchedClass::RealTime { .. } => {
                    if flags == UpdateFlags::Tick {
                        if let Some(data) = curr.data().downcast_ref::<TaskData>() {
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
                        TaskData::set_vruntime(curr, vruntime + vruntime_delta);

                        if let Some(data) = curr.data().downcast_ref::<TaskData>() {
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
            }
        }

        self.vtime = self.vtime.max(self.min_vruntime());

        match flags {
            UpdateFlags::Tick => self.should_preempt_current(),
            UpdateFlags::Wait | UpdateFlags::Yield | UpdateFlags::Exit => self.nr_runnable > 0,
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
        } else {
            return None;
        };

        if let Some(prev_task) = self.current.replace(next_task) {
            self.enqueue_task(prev_task);
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
