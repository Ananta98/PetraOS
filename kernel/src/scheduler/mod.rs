use crate::proc::pid_table::Pid;
use crate::proc::tid_table::Tid;
use crate::scheduler::nice::NiceWeight;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use ostd::cpu::{CpuId, PinCurrentCpu, num_cpus};
use ostd::sync::SpinLock;
use ostd::task::scheduler::info::CommonSchedInfo;
use ostd::task::scheduler::{EnqueueFlags, LocalRunQueue, Scheduler, UpdateFlags};
use ostd::task::{Task, disable_preempt};
use ostd::util::id_set::Id;

pub mod fair;
pub mod nice;
pub mod real_time;

use fair::FairRunQueue;
use real_time::RtRunQueue;

pub fn nice_to_weight(nice: i32) -> u64 {
    NiceWeight::new(nice).to_weight()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedClass {
    RealTime { priority: u32 }, // Higher value is higher priority
    Fair { nice: NiceWeight },
}

/// Per-task scheduling metadata attached to every `ostd::task::Task`.
pub struct TaskData {
    /// Scheduling class and parameters.
    pub class: SchedClass,
    /// Accumulated virtual runtime (nanoseconds, CFS bookkeeping).
    pub vruntime: AtomicU64,
    /// Infinity Scheduler: Exponential Moving Average for execution slices.
    pub ema: AtomicU64,
    /// Last time this task was dequeued (vtime).
    pub last_dequeue_vtime: AtomicU64,
    /// Owning process identifier.
    pub pid: Pid,
    /// This thread's unique identifier.
    pub tid: Tid,
}

impl TaskData {
    /// Create `TaskData` with an explicit scheduling class, `Pid`, and `Tid`.
    pub fn new(class: SchedClass, pid: Pid, tid: Tid) -> Self {
        Self {
            class,
            vruntime: AtomicU64::new(0),
            ema: AtomicU64::new(0),
            last_dequeue_vtime: AtomicU64::new(0),
            pid,
            tid,
        }
    }
}

pub(crate) fn get_sched_data(task: &Task) -> (SchedClass, u64) {
    if let Some(data) = task.data().downcast_ref::<TaskData>() {
        (data.class, data.vruntime.load(Ordering::Relaxed))
    } else {
        (
            SchedClass::Fair {
                nice: NiceWeight::new(0),
            },
            0,
        )
    }
}

pub(crate) fn set_vruntime(task: &Task, vruntime: u64) {
    if let Some(data) = task.data().downcast_ref::<TaskData>() {
        data.vruntime.store(vruntime, Ordering::Relaxed);
    }
}

pub(crate) fn get_weight(class: SchedClass) -> u64 {
    match class {
        SchedClass::RealTime { .. } => 0,
        SchedClass::Fair { nice } => nice.to_weight(),
    }
}

pub(crate) fn get_deadline(vruntime: u64, class: SchedClass, task: Option<&Task>) -> u64 {
    let mut weight = get_weight(class);
    if let Some(task) = task {
        if let Some(data) = task.data().downcast_ref::<TaskData>() {
            let ema = data.ema.load(Ordering::Relaxed);
            let ema_pct = (ema * 100 / 2_000_000).min(100);
            let weight_factor = 100 - ema_pct * 75 / 100;
            weight = weight * 100 / weight_factor.max(1);
        }
    }
    vruntime + 1024_000 / weight.max(1)
}

pub struct RunQueue {
    current: Option<Arc<Task>>,
    rt: RtRunQueue,
    fair: FairRunQueue,
    vtime: u64,
    nr_runnable: usize,
}

impl RunQueue {
    pub const fn new() -> Self {
        Self {
            current: None,
            rt: RtRunQueue::new(),
            fair: FairRunQueue::new(),
            vtime: 0,
            nr_runnable: 0,
        }
    }

    pub fn min_vruntime(&self) -> u64 {
        let mut min_val = if let Some(curr) = &self.current {
            let (class, vruntime) = get_sched_data(curr);
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
            let (class, _) = get_sched_data(curr);
            if let SchedClass::Fair { nice } = class {
                total += nice.to_weight();
            }
        }
        total
    }

    pub fn enqueue_task(&mut self, task: Arc<Task>) {
        let (class, vruntime) = get_sched_data(&task);
        match class {
            SchedClass::RealTime { priority } => {
                let mut effective_priority = priority;
                if let Some(data) = task.data().downcast_ref::<TaskData>() {
                    let ema = data.ema.load(Ordering::Relaxed);
                    let decay = (ema * 10 / 2_000_000) as u32; // Drop up to 10 prio levels based on CPU usage
                    effective_priority = priority.saturating_sub(decay);
                }
                self.rt.enqueue(task, effective_priority);
            }
            SchedClass::Fair { .. } => {
                self.fair.enqueue(task, vruntime, self.vtime);
            }
        }
        self.nr_runnable += 1;
        self.vtime = self.vtime.max(self.min_vruntime());
    }

    pub fn should_preempt_current(&self) -> bool {
        let Some(curr) = &self.current else {
            return false;
        };

        let (curr_class, curr_vruntime) = get_sched_data(curr);

        match curr_class {
            SchedClass::RealTime {
                priority: curr_priority,
            } => {
                if let Some(highest_priority) = self.rt.highest_priority() {
                    if highest_priority > curr_priority {
                        return true;
                    }
                }
                false
            }
            SchedClass::Fair { .. } => {
                if !self.rt.is_empty() {
                    return true;
                }

                if self.fair.is_empty() {
                    return false;
                }

                let curr_deadline = get_deadline(curr_vruntime, curr_class, Some(&**curr));
                for (&vruntime, queue) in self.fair.tasks.range(..=self.vtime) {
                    for task in queue {
                        let (class, _) = get_sched_data(task);
                        let deadline = get_deadline(vruntime, class, Some(&**task));
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

impl LocalRunQueue<Task> for RunQueue {
    fn current(&self) -> Option<&Arc<Task>> {
        self.current.as_ref()
    }

    fn update_current(&mut self, flags: UpdateFlags) -> bool {
        if let Some(curr) = &self.current {
            let (class, vruntime) = get_sched_data(curr);
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
                        set_vruntime(curr, vruntime + vruntime_delta);

                        // Infinity EMA: ema += (BUDGET_MAX - ema) * delta_ns * 16 / (BUDGET_MAX * 256)
                        if let Some(data) = curr.data().downcast_ref::<TaskData>() {
                            let ema = data.ema.load(Ordering::Relaxed);
                            let budget_max: u64 = 2_000_000;
                            let delta_ns: u64 = 1_000_000; // Fake 1ms
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
        let next_task = if !self.rt.is_empty() {
            let task = self.rt.pick_next().unwrap();
            self.nr_runnable -= 1;
            task
        } else if !self.fair.is_empty() {
            let task = self.fair.pick_next(self.vtime).unwrap();
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

pub struct CombinedScheduler {
    rq: Vec<SpinLock<RunQueue>>,
}

impl CombinedScheduler {
    pub fn new(nr_cpus: usize) -> Self {
        let mut rq = Vec::new();
        for _ in 0..nr_cpus {
            rq.push(SpinLock::new(RunQueue::new()));
        }
        Self { rq }
    }

    fn select_cpu(&self) -> CpuId {
        let mut min_load = usize::MAX;
        let mut best_cpu_idx = 0;
        for (cpu_idx, rq_lock) in self.rq.iter().enumerate() {
            let load = {
                let rq = rq_lock.disable_irq().lock();
                rq.nr_runnable
            };
            if load < min_load {
                min_load = load;
                best_cpu_idx = cpu_idx;
            }
        }
        CpuId::new(best_cpu_idx as u32)
    }
}

impl Default for CombinedScheduler {
    fn default() -> Self {
        Self::new(num_cpus())
    }
}

impl Scheduler<Task> for CombinedScheduler {
    fn enqueue(&self, runnable: Arc<Task>, flags: EnqueueFlags) -> Option<CpuId> {
        let (still_in_rq, target_cpu) = {
            let selected_cpu_id = if flags == EnqueueFlags::Spawn {
                self.select_cpu()
            } else {
                CpuId::current_racy()
            };

            if let Err(task_cpu_id) = runnable.cpu().set_if_is_none(selected_cpu_id) {
                debug_assert!(flags != EnqueueFlags::Spawn);
                (true, task_cpu_id)
            } else {
                (false, selected_cpu_id)
            }
        };

        let mut rq = self.rq[target_cpu.as_usize()].disable_irq().lock();
        if still_in_rq && let Err(_) = runnable.cpu().set_if_is_none(target_cpu) {
            return None;
        }

        rq.enqueue_task(runnable);

        if rq.should_preempt_current() {
            Some(target_cpu)
        } else {
            None
        }
    }

    fn local_rq_with(&self, f: &mut dyn FnMut(&dyn LocalRunQueue<Task>)) {
        let preempt_guard = disable_preempt();
        let guard = self.rq[preempt_guard.current_cpu().as_usize()]
            .disable_irq()
            .lock();
        f(&*guard);
    }

    fn mut_local_rq_with(&self, f: &mut dyn FnMut(&mut dyn LocalRunQueue<Task>)) {
        let preempt_guard = disable_preempt();
        let mut guard = self.rq[preempt_guard.current_cpu().as_usize()]
            .disable_irq()
            .lock();
        f(&mut *guard);
    }
}

pub fn init() {
    let scheduler = Box::new(CombinedScheduler::default());
    let scheduler_ref = Box::leak(scheduler);
    ostd::task::scheduler::inject_scheduler(scheduler_ref);
    ostd::task::scheduler::enable_preemption_on_cpu();
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::proc::pid_table::Pid;
    use crate::proc::tid_table::Tid;
    use ostd::prelude::*;
    use ostd::task::TaskOptions;

    #[ktest]
    fn test_fair_runqueue_uses_eevdf_deadline_order() {
        let mut rq = RunQueue::new();

        let task_fast = Arc::new(
            TaskOptions::new(|| {})
                .data(TaskData::new(
                    SchedClass::Fair {
                        nice: NiceWeight::new(0),
                    },
                    Pid::from_raw(1),
                    Tid::from_raw(1),
                ))
                .build()
                .unwrap(),
        );

        let task_slow = Arc::new(
            TaskOptions::new(|| {})
                .data(TaskData::new(
                    SchedClass::Fair {
                        nice: NiceWeight::new(3),
                    },
                    Pid::from_raw(1),
                    Tid::from_raw(2),
                ))
                .build()
                .unwrap(),
        );

        rq.enqueue_task(task_fast.clone());
        rq.enqueue_task(task_slow.clone());

        let picked = rq.try_pick_next().unwrap();
        assert!(Arc::ptr_eq(picked, &task_fast));

        let picked = rq.try_pick_next().unwrap();
        assert!(Arc::ptr_eq(picked, &task_slow));
    }

    #[ktest]
    fn test_preempt_current_when_a_newer_task_has_earlier_deadline() {
        let mut rq = RunQueue::new();

        let current = Arc::new(
            TaskOptions::new(|| {})
                .data(TaskData::new(
                    SchedClass::Fair {
                        nice: NiceWeight::new(0),
                    },
                    Pid::from_raw(1),
                    Tid::from_raw(3),
                ))
                .build()
                .unwrap(),
        );
        let newcomer = Arc::new(
            TaskOptions::new(|| {})
                .data(TaskData::new(
                    SchedClass::Fair {
                        nice: NiceWeight::new(0),
                    },
                    Pid::from_raw(1),
                    Tid::from_raw(4),
                ))
                .build()
                .unwrap(),
        );

        set_vruntime(&current, 1500);
        rq.current = Some(current.clone());
        rq.vtime = 0;
        rq.enqueue_task(newcomer.clone());

        assert!(rq.should_preempt_current());
    }

    #[ktest]
    fn test_rt_preempts_fair() {
        let mut rq = RunQueue::new();

        let task_fair = Arc::new(
            TaskOptions::new(|| {})
                .data(TaskData::new(
                    SchedClass::Fair {
                        nice: NiceWeight::new(0),
                    },
                    Pid::from_raw(1),
                    Tid::from_raw(10),
                ))
                .build()
                .unwrap(),
        );

        let task_rt = Arc::new(
            TaskOptions::new(|| {})
                .data(TaskData::new(
                    SchedClass::RealTime { priority: 1 },
                    Pid::from_raw(1),
                    Tid::from_raw(11),
                ))
                .build()
                .unwrap(),
        );

        rq.enqueue_task(task_fair.clone());
        rq.enqueue_task(task_rt.clone());

        // RT task must be picked first regardless of fair task vruntime/deadlines
        let picked = rq.try_pick_next().unwrap();
        assert!(Arc::ptr_eq(picked, &task_rt));

        let picked = rq.try_pick_next().unwrap();
        assert!(Arc::ptr_eq(picked, &task_fair));
    }

    #[ktest]
    fn test_rt_priority_preemption() {
        let mut rq = RunQueue::new();

        let task_rt_low = Arc::new(
            TaskOptions::new(|| {})
                .data(TaskData::new(
                    SchedClass::RealTime { priority: 1 },
                    Pid::from_raw(1),
                    Tid::from_raw(12),
                ))
                .build()
                .unwrap(),
        );

        let task_rt_high = Arc::new(
            TaskOptions::new(|| {})
                .data(TaskData::new(
                    SchedClass::RealTime { priority: 10 },
                    Pid::from_raw(1),
                    Tid::from_raw(13),
                ))
                .build()
                .unwrap(),
        );

        rq.enqueue_task(task_rt_low.clone());
        rq.enqueue_task(task_rt_high.clone());

        // Higher priority RT task picked first
        let picked = rq.try_pick_next().unwrap();
        assert!(Arc::ptr_eq(picked, &task_rt_high));

        let picked = rq.try_pick_next().unwrap();
        assert!(Arc::ptr_eq(picked, &task_rt_low));
    }
}
