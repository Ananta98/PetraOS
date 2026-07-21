use super::{TaskData, get_deadline, get_sched_data, set_vruntime};
use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::sync::atomic::Ordering;
use ostd::task::Task;

/// Run queue logic dedicated to EEVDF (Earliest Eligible Virtual Deadline First) scheduling.
pub struct FairRunQueue {
    pub tasks: BTreeMap<u64, VecDeque<Arc<Task>>>,
}

impl FairRunQueue {
    /// Create a new empty `FairRunQueue`.
    pub const fn new() -> Self {
        Self {
            tasks: BTreeMap::new(),
        }
    }

    /// Enqueue a fair/CFS task.
    pub fn enqueue(&mut self, task: Arc<Task>, vruntime: u64, vtime: u64) {
        if let Some(data) = task.data().downcast_ref::<TaskData>() {
            let last_vtime = data.last_dequeue_vtime.load(Ordering::Relaxed);
            let sleep_ns = vtime.saturating_sub(last_vtime);
            if sleep_ns > 0 {
                let ema = data.ema.load(Ordering::Relaxed);
                // dec = min(sleep_ns * 16 * 256 / 20_000_000, 256)
                let dec = (sleep_ns * 16 * 256 / 20_000_000).min(256);
                let new_ema = ema - ema * dec / 256;
                data.ema.store(new_ema, Ordering::Relaxed);
            }
        }
        let new_vruntime = vruntime.max(vtime);
        set_vruntime(&task, new_vruntime);
        self.tasks
            .entry(new_vruntime)
            .or_insert_with(VecDeque::new)
            .push_back(task);
    }

    /// Pick the earliest eligible task with the minimum virtual deadline.
    pub fn pick_next(&mut self, vtime: u64) -> Option<Arc<Task>> {
        if self.tasks.is_empty() {
            return None;
        }

        let mut best_key = None;
        let mut best_deque_idx = None;
        let mut best_deadline = u64::MAX;

        for (&vruntime, queue) in self.tasks.range(..=vtime) {
            for (dq_idx, task) in queue.iter().enumerate() {
                let (class, _) = get_sched_data(task);
                let deadline = get_deadline(vruntime, class, Some(&**task));
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
                let (&min_vruntime, _) = self.tasks.iter().next().unwrap();
                (min_vruntime, 0)
            };

        let queue = self.tasks.get_mut(&key_to_remove).unwrap();
        let task = queue.remove(deque_idx_to_remove).unwrap();
        if queue.is_empty() {
            self.tasks.remove(&key_to_remove);
        }
        Some(task)
    }

    /// Get the minimum virtual runtime of all queued tasks.
    pub fn min_vruntime(&self) -> Option<u64> {
        self.tasks.keys().next().copied()
    }

    /// Calculate the sum of weights of all fair tasks in the queue.
    pub fn total_weight(&self) -> u64 {
        let mut total = 0;
        for queue in self.tasks.values() {
            for task in queue {
                let (class, _) = get_sched_data(task);
                if let super::SchedClass::Fair { nice } = class {
                    let base_weight = nice.to_weight();
                    if let Some(data) = task.data().downcast_ref::<TaskData>() {
                        let ema = data.ema.load(Ordering::Relaxed);
                        let ema_pct = (ema * 100 / 2_000_000).min(100);
                        let weight_factor = 100 - ema_pct * 75 / 100;
                        total += base_weight * 100 / weight_factor.max(1);
                    } else {
                        total += base_weight;
                    }
                }
            }
        }
        total
    }

    /// Check if there are no fair tasks.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}
