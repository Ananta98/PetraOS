//! Real-Time (RT) scheduling class implementation (FIFO / Priority-based).

use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use ostd::task::Task;

use super::policy::SchedClassPolicy;
use crate::scheduler::{SchedClass, get_sched_data};

/// Run queue logic dedicated to Real-Time scheduling.
#[derive(Debug)]
pub struct RtRunQueue {
    tasks: BTreeMap<u32, VecDeque<Arc<Task>>>,
}

impl RtRunQueue {
    /// Create a new empty `RtRunQueue`.
    pub const fn new() -> Self {
        Self {
            tasks: BTreeMap::new(),
        }
    }

    /// Enqueue a real-time task with a given priority.
    pub fn enqueue_prio(&mut self, task: Arc<Task>, priority: u32) {
        self.tasks
            .entry(priority)
            .or_insert_with(VecDeque::new)
            .push_back(task);
    }

    /// Retrieve the highest active priority in the queue.
    pub fn highest_priority(&self) -> Option<u32> {
        self.tasks
            .iter()
            .rev()
            .find(|(_, queue)| !queue.is_empty())
            .map(|(&priority, _)| priority)
    }

    /// Return the total number of real-time tasks.
    pub fn len(&self) -> usize {
        self.tasks.values().map(|q| q.len()).sum()
    }

    /// Check if there are no real-time tasks.
    pub fn is_empty(&self) -> bool {
        self.tasks.values().all(|q| q.is_empty())
    }
}

/// Real-Time scheduling class policy wrapper.
#[derive(Debug)]
pub struct RtSchedClass;

impl SchedClassPolicy for RtRunQueue {
    fn enqueue(&mut self, task: Arc<Task>, _vtime: u64) {
        let (class, _) = get_sched_data(&task);
        if let SchedClass::RealTime { priority } = class {
            self.enqueue_prio(task, priority);
        }
    }

    fn pick_next(&mut self, _vtime: u64) -> Option<Arc<Task>> {
        let highest_priority = self.highest_priority()?;
        let queue = self.tasks.get_mut(&highest_priority)?;
        let task = queue.pop_front()?;
        if queue.is_empty() {
            self.tasks.remove(&highest_priority);
        }
        Some(task)
    }

    fn check_preempt_curr(&self, curr: &Task, newcomer: &Task, _vtime: u64) -> bool {
        let (curr_class, _) = get_sched_data(curr);
        let (new_class, _) = get_sched_data(newcomer);

        match (curr_class, new_class) {
            (SchedClass::Fair { .. }, SchedClass::RealTime { .. }) => true,
            (
                SchedClass::RealTime {
                    priority: curr_prio,
                },
                SchedClass::RealTime { priority: new_prio },
            ) => new_prio > curr_prio,
            _ => false,
        }
    }

    fn len(&self) -> usize {
        self.tasks.values().map(|q| q.len()).sum()
    }
}
