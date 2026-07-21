use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use ostd::task::Task;

/// Run queue logic dedicated to Real-Time scheduling.
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

    /// Enqueue a real-time task.
    pub fn enqueue(&mut self, task: Arc<Task>, priority: u32) {
        self.tasks
            .entry(priority)
            .or_insert_with(VecDeque::new)
            .push_back(task);
    }

    /// Pick the highest priority real-time task.
    pub fn pick_next(&mut self) -> Option<Arc<Task>> {
        let highest_priority = self.highest_priority()?;
        let queue = self.tasks.get_mut(&highest_priority).unwrap();
        let task = queue.pop_front().unwrap();
        if queue.is_empty() {
            self.tasks.remove(&highest_priority);
        }
        Some(task)
    }

    /// Retrieve the highest active priority.
    pub fn highest_priority(&self) -> Option<u32> {
        self.tasks
            .iter()
            .rev()
            .find(|(_, queue)| !queue.is_empty())
            .map(|(&priority, _)| priority)
    }

    /// Check if there are no real-time tasks.
    pub fn is_empty(&self) -> bool {
        self.tasks.values().all(|q| q.is_empty())
    }
}
