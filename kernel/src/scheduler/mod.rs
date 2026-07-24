//! Process and thread scheduler subsystem.
//!
//! Provides the top-level `ClassScheduler` managing per-CPU runqueues
//! (`PerCpuClassRqSet`) and task scheduling policies (EEVDF/Fair, Real-Time).

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use ostd::cpu::{CpuId, PinCurrentCpu, num_cpus};
use ostd::sync::SpinLock;
use ostd::task::scheduler::info::CommonSchedInfo;
use ostd::task::scheduler::{
    EnqueueFlags, LocalRunQueue, Scheduler, enable_preemption_on_cpu, inject_scheduler,
};
use ostd::task::{Task, disable_preempt};
use ostd::util::id_set::Id;

use crate::proc::pid_table::Pid;
use crate::proc::tid_table::Tid;
use crate::scheduler::nice::NiceWeight;

pub mod fair;
pub mod nice;
pub mod policy;
pub mod real_time;
pub mod runqueue;
pub mod task_data;

pub use crate::scheduler::task_data::TaskData;
pub use fair::FairRunQueue;
pub use policy::SchedClassPolicy;
pub use real_time::{RtRunQueue, RtSchedClass};
pub use runqueue::PerCpuClassRqSet;

/// Alias for `TaskData::sched_data`.
pub fn get_sched_data(task: &ostd::task::Task) -> (SchedClass, u64) {
    TaskData::sched_data(task)
}

pub fn nice_to_weight(nice: i32) -> u64 {
    NiceWeight::new(nice).to_weight()
}

/// Scheduling class assigned to a task (Real-Time vs. Fair/EEVDF).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedClass {
    RealTime { priority: u32 }, // Higher value is higher priority
    Fair { nice: NiceWeight },
}

impl SchedClass {
    /// Return the weight associated with this scheduling class.
    pub fn weight(&self) -> u64 {
        match self {
            Self::RealTime { .. } => 0,
            Self::Fair { nice } => nice.to_weight(),
        }
    }
}

/// Top-level multi-core scheduler managing per-CPU runqueue sets (`PerCpuClassRqSet`).
pub struct ClassScheduler {
    rqs: Vec<SpinLock<PerCpuClassRqSet>>,
}

impl ClassScheduler {
    /// Create a new `ClassScheduler` configured for `nr_cpus` cores.
    pub fn new(nr_cpus: usize) -> Self {
        let mut rqs = Vec::with_capacity(nr_cpus);
        for _ in 0..nr_cpus {
            rqs.push(SpinLock::new(PerCpuClassRqSet::new()));
        }
        Self { rqs }
    }

    /// Select the best CPU core to place a new task based on minimum runqueue load.
    fn select_cpu(&self) -> CpuId {
        let mut min_load = usize::MAX;
        let mut best_cpu_idx = 0;
        for (cpu_idx, rq_lock) in self.rqs.iter().enumerate() {
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

impl Default for ClassScheduler {
    fn default() -> Self {
        Self::new(num_cpus())
    }
}

impl Scheduler<Task> for ClassScheduler {
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

        let mut rq = self.rqs[target_cpu.as_usize()].disable_irq().lock();
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
        let guard = self.rqs[preempt_guard.current_cpu().as_usize()]
            .disable_irq()
            .lock();
        f(&*guard);
    }

    fn mut_local_rq_with(&self, f: &mut dyn FnMut(&mut dyn LocalRunQueue<Task>)) {
        let preempt_guard = disable_preempt();
        let mut guard = self.rqs[preempt_guard.current_cpu().as_usize()]
            .disable_irq()
            .lock();
        f(&mut *guard);
    }
}

/// Initialize the scheduler subsystem by injecting the per-CPU `ClassScheduler`.
pub fn init() {
    let scheduler = Box::new(ClassScheduler::default());
    let scheduler_ref = Box::leak(scheduler);
    inject_scheduler(scheduler_ref);
    enable_preemption_on_cpu();
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::proc::pid_table::Pid;
    use crate::proc::tid_table::Tid;
    use crate::scheduler::nice::NiceWeight;
    use ostd::prelude::*;
    use ostd::task::TaskOptions;

    #[ktest]
    fn test_fair_runqueue_uses_eevdf_deadline_order() {
        let mut rq = PerCpuClassRqSet::new();

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
        let mut rq = PerCpuClassRqSet::new();

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

        TaskData::set_vruntime(&current, 1500);

        rq.current = Some(current.clone());
        rq.vtime = 0;
        rq.enqueue_task(newcomer.clone());

        assert!(rq.should_preempt_current());
    }

    #[ktest]
    fn test_rt_preempts_fair() {
        let mut rq = PerCpuClassRqSet::new();

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

        let picked = rq.try_pick_next().unwrap();
        assert!(Arc::ptr_eq(picked, &task_rt));

        let picked = rq.try_pick_next().unwrap();
        assert!(Arc::ptr_eq(picked, &task_fair));
    }

    #[ktest]
    fn test_rt_priority_preemption() {
        let mut rq = PerCpuClassRqSet::new();

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

        let picked = rq.try_pick_next().unwrap();
        assert!(Arc::ptr_eq(picked, &task_rt_high));

        let picked = rq.try_pick_next().unwrap();
        assert!(Arc::ptr_eq(picked, &task_rt_low));
    }
}
