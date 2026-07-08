use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use ostd::cpu::{CpuId, PinCurrentCpu, num_cpus};
use ostd::sync::SpinLock;
use ostd::task::scheduler::info::CommonSchedInfo;
use ostd::task::scheduler::{EnqueueFlags, LocalRunQueue, Scheduler, UpdateFlags};
use ostd::task::{Task, disable_preempt};
use ostd::util::id_set::Id;

/// Linux/Unix priority-to-weight conversion table for nice values [-20 .. 19].
const SCHED_NICE_TO_WEIGHT: [u64; 40] = [
 /* -20 */     88761,     71755,     56483,     46273,     36291,
 /* -15 */     29154,     23254,     18705,     14949,     11916,
 /* -10 */      9548,      7620,      6100,      4904,      3906,
 /*  -5 */      3121,      2501,      1991,      1586,      1277,
 /*   0 */      1024,       820,       655,       526,       414,
 /*   5 */       335,       272,       215,       172,       137,
 /*  10 */       110,        87,        70,        56,        45,
 /*  15 */        36,        29,        23,        18,        15,
];

/// Helper to convert a nice value (-20 to 19) to standard CFS weight.
pub fn nice_to_weight(nice: i32) -> u64 {
    let index = (nice.clamp(-20, 19) + 20) as usize;
    SCHED_NICE_TO_WEIGHT[index]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedClass {
    Fair { nice: i32 },
}

pub struct TaskData {
    pub class: SchedClass,
    pub vruntime: AtomicU64,
}

impl TaskData {
    pub fn new(class: SchedClass) -> Self {
        Self {
            class,
            vruntime: AtomicU64::new(0),
        }
    }
}

fn get_sched_data(task: &Task) -> (SchedClass, u64) {
    if let Some(data) = task.data().downcast_ref::<TaskData>() {
        (data.class, data.vruntime.load(Ordering::Relaxed))
    } else {
        (SchedClass::Fair { nice: 0 }, 0)
    }
}

fn set_vruntime(task: &Task, vruntime: u64) {
    if let Some(data) = task.data().downcast_ref::<TaskData>() {
        data.vruntime.store(vruntime, Ordering::Relaxed);
    }
}

fn get_weight(class: SchedClass) -> u64 {
    match class {
        SchedClass::Fair { nice } => nice_to_weight(nice),
    }
}

fn get_deadline(vruntime: u64, class: SchedClass) -> u64 {
    let weight = get_weight(class);
    vruntime + 1024_000 / weight.max(1)
}

pub struct RunQueue {
    current: Option<Arc<Task>>,
    cfs_tasks: BTreeMap<u64, VecDeque<Arc<Task>>>,
    vtime: u64,
    nr_runnable: usize,
}

impl RunQueue {
    pub const fn new() -> Self {
        Self {
            current: None,
            cfs_tasks: BTreeMap::new(),
            vtime: 0,
            nr_runnable: 0,
        }
    }

    pub fn min_vruntime(&self) -> u64 {
        let mut min_val = if let Some(curr) = &self.current {
            let (_, vruntime) = get_sched_data(curr);
            vruntime
        } else {
            0
        };
        if let Some((&vruntime, _)) = self.cfs_tasks.iter().next() {
            if min_val == 0 {
                min_val = vruntime;
            } else {
                min_val = min_val.min(vruntime);
            }
        }
        min_val
    }

    fn total_fair_weight(&self) -> u64 {
        let mut total = 0;
        if let Some(curr) = &self.current {
            total += get_weight(get_sched_data(curr).0);
        }
        for queue in self.cfs_tasks.values() {
            for task in queue {
                total += get_weight(get_sched_data(task).0);
            }
        }
        total
    }

    pub fn enqueue_task(&mut self, task: Arc<Task>) {
        let (_, vruntime) = get_sched_data(&task);
        let new_vruntime = vruntime.max(self.vtime);
        set_vruntime(&task, new_vruntime);
        self.cfs_tasks
            .entry(new_vruntime)
            .or_insert_with(VecDeque::new)
            .push_back(task);
        self.nr_runnable += 1;
        self.vtime = self.vtime.max(self.min_vruntime());
    }

    pub fn should_preempt_current(&self) -> bool {
        let Some(curr) = &self.current else {
            return false;
        };

        if self.cfs_tasks.is_empty() {
            return false;
        }

        let (curr_class, curr_vruntime) = get_sched_data(curr);
        let curr_deadline = get_deadline(curr_vruntime, curr_class);

        for (&vruntime, queue) in self.cfs_tasks.range(..=self.vtime) {
            for task in queue {
                let (class, _) = get_sched_data(task);
                let deadline = get_deadline(vruntime, class);
                if deadline + 1000 < curr_deadline {
                    return true;
                }
            }
        }

        false
    }
}

impl LocalRunQueue<Task> for RunQueue {
    fn current(&self) -> Option<&Arc<Task>> {
        self.current.as_ref()
    }

    fn update_current(&mut self, flags: UpdateFlags) -> bool {
        if let Some(curr) = &self.current {
            if flags == UpdateFlags::Tick {
                let (class, vruntime) = get_sched_data(curr);
                let weight = get_weight(class);
                let delta = 1000;
                let vruntime_delta = delta * 1024 / weight.max(1);
                set_vruntime(curr, vruntime + vruntime_delta);

                let total_w = self.total_fair_weight().max(1);
                let vtime_delta = delta * 1024 / total_w;
                self.vtime += vtime_delta;
            }
        }

        self.vtime = self.vtime.max(self.min_vruntime());

        match flags {
            UpdateFlags::Tick => self.should_preempt_current(),
            UpdateFlags::Wait | UpdateFlags::Yield | UpdateFlags::Exit => self.nr_runnable > 0,
        }
    }

    fn try_pick_next(&mut self) -> Option<&Arc<Task>> {
        let next_task = if !self.cfs_tasks.is_empty() {
            let mut best_key = None;
            let mut best_deque_idx = None;
            let mut best_deadline = u64::MAX;

            for (&vruntime, queue) in self.cfs_tasks.range(..=self.vtime) {
                for (dq_idx, task) in queue.iter().enumerate() {
                    let (class, _) = get_sched_data(task);
                    let deadline = get_deadline(vruntime, class);
                    if deadline < best_deadline {
                        best_deadline = deadline;
                        best_key = Some(vruntime);
                        best_deque_idx = Some(dq_idx);
                    }
                }
            }

            let (key_to_remove, deque_idx_to_remove) =
                if let (Some(k), Some(idx)) = (best_key, best_deque_idx) {
                    (k, idx)
                } else {
                    let (&min_vruntime, _) = self.cfs_tasks.iter().next().unwrap();
                    (min_vruntime, 0)
                };

            let queue = self.cfs_tasks.get_mut(&key_to_remove).unwrap();
            let task = queue.remove(deque_idx_to_remove).unwrap();
            if queue.is_empty() {
                self.cfs_tasks.remove(&key_to_remove);
            }
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
        self.vtime = self.vtime.max(self.min_vruntime());
        res
    }
}

pub struct EevdfScheduler {
    rq: Vec<SpinLock<RunQueue>>,
}

impl EevdfScheduler {
    pub fn new(nr_cpus: usize) -> Self {
        let mut rq = Vec::new();
        for _ in 0..nr_cpus {
            rq.push(SpinLock::new(RunQueue::new()));
        }
        Self { rq }
    }

    fn select_cpu(&self) -> CpuId {
        CpuId::bsp()
    }
}

impl Default for EevdfScheduler {
    fn default() -> Self {
        Self::new(num_cpus())
    }
}

impl Scheduler<Task> for EevdfScheduler {
    fn enqueue(&self, runnable: Arc<Task>, flags: EnqueueFlags) -> Option<CpuId> {
        let (still_in_rq, target_cpu) = {
            let selected_cpu_id = self.select_cpu();

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
    let scheduler = Box::new(EevdfScheduler::default());
    let scheduler_ref = Box::leak(scheduler);
    ostd::task::scheduler::inject_scheduler(scheduler_ref);
    ostd::task::scheduler::enable_preemption_on_cpu();
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::*;
    use ostd::task::TaskOptions;

    #[ktest]
    fn test_fair_runqueue_uses_eevdf_deadline_order() {
        let mut rq = RunQueue::new();

        let task_fast = Arc::new(
            TaskOptions::new(|| {})
                .data(TaskData::new(SchedClass::Fair { nice: 0 }))
                .build()
                .unwrap(),
        );

        let task_slow = Arc::new(
            TaskOptions::new(|| {})
                .data(TaskData::new(SchedClass::Fair { nice: 3 }))
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
                .data(TaskData::new(SchedClass::Fair { nice: 0 }))
                .build()
                .unwrap(),
        );
        let newcomer = Arc::new(
            TaskOptions::new(|| {})
                .data(TaskData::new(SchedClass::Fair { nice: 0 }))
                .build()
                .unwrap(),
        );

        set_vruntime(&current, 1500);
        rq.current = Some(current.clone());
        rq.vtime = 0;
        rq.enqueue_task(newcomer.clone());

        assert!(rq.should_preempt_current());
    }
}
