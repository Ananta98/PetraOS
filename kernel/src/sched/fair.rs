use super::stats::SchedulerStats;
use crate::proc::thread::{Thread, ThreadId, ThreadState};
use crate::sync::spinlock::Spinlock;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;

/// Weight corresponding to `nice = 0` in the CFS weight table.
pub const NICE_0_WEIGHT: u32 = 1024;
pub const MAX_CPUS: usize = 8;

/// The Completely Fair Scheduler (CFS).
pub struct Scheduler {
    /// The run queue, ordered by virtual runtime (vruntime) and ThreadId.
    /// BTreeMap provides O(log N) insertion and extraction of the thread with the minimum vruntime.
    run_queue: BTreeMap<(u64, ThreadId), Arc<Spinlock<Thread>>>,

    /// The currently running threads per CPU.
    pub current_threads: [Option<Arc<Spinlock<Thread>>>; MAX_CPUS],

    /// The minimum vruntime across all threads in the run queue.
    /// Used to initialize new threads so they don't get scheduled indefinitely.
    min_vruntime: u64,

    /// Scheduler metrics and statistics.
    pub stats: SchedulerStats,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            run_queue: BTreeMap::new(),
            current_threads: [const { None }; MAX_CPUS],
            min_vruntime: 0,
            stats: SchedulerStats::new(),
        }
    }

    /// Adds a thread to the run queue.
    pub fn add_thread(&mut self, thread: Arc<Spinlock<Thread>>) {
        let mut t_lock = thread.lock();

        // Ensure new threads get a baseline vruntime so they aren't overly prioritized.
        if t_lock.vruntime < self.min_vruntime {
            t_lock.vruntime = self.min_vruntime;
        }

        t_lock.state = ThreadState::Ready;
        let vruntime = t_lock.vruntime;
        let tid = t_lock.tid;
        drop(t_lock); // Release lock before moving thread into the map

        self.run_queue.insert((vruntime, tid), thread);
        self.stats.inc_threads_added();
    }

    /// Removes a thread from the run queue by its ThreadId.
    pub fn remove_thread(&mut self, tid: ThreadId) -> Option<Arc<Spinlock<Thread>>> {
        // Find the key matching the ThreadId
        let key_to_remove = self.run_queue.keys().find(|k| k.1 == tid).copied();

        if let Some(key) = key_to_remove {
            let removed = self.run_queue.remove(&key);
            if removed.is_some() {
                self.stats.inc_threads_removed();
            }
            removed
        } else {
            None
        }
    }

    /// Picks the next thread to run, removing it from the run queue, for a specific CPU.
    pub fn pick_next(&mut self, cpu_id: u32) -> Option<Arc<Spinlock<Thread>>> {
        if let Some((key, thread)) = self.run_queue.pop_first() {
            self.min_vruntime = key.0; // Update min_vruntime
            self.current_threads[cpu_id as usize] = Some(thread.clone());

            let mut t_lock = thread.lock();
            t_lock.state = ThreadState::Running;
            drop(t_lock);

            Some(thread)
        } else {
            self.current_threads[cpu_id as usize] = None;
            None
        }
    }

    /// Updates the vruntime of the currently running thread on `cpu_id`.
    /// `delta_ns` is the time elapsed since the last tick (e.g., in nanoseconds).
    pub fn tick(&mut self, cpu_id: u32, delta_ns: u64) {
        self.stats.inc_ticks();
        if let Some(thread) = &self.current_threads[cpu_id as usize] {
            let mut t_lock = thread.lock();

            // Calculate virtual runtime increment:
            // vruntime += delta_ns * (NICE_0_WEIGHT / weight)
            let weight = if t_lock.weight > 0 {
                t_lock.weight
            } else {
                NICE_0_WEIGHT
            };
            let vruntime_delta = (delta_ns * NICE_0_WEIGHT as u64) / weight as u64;

            t_lock.vruntime += vruntime_delta;
        }
    }

    /// Voluntarily yield the CPU for `cpu_id`.
    pub fn yield_current(&mut self, cpu_id: u32) {
        self.stats.inc_yields();
        if let Some(thread) = self.current_threads[cpu_id as usize].take() {
            self.add_thread(thread);
        }
    }

    /// Blocks the current thread on `cpu_id` (removes it from CPU, doesn't put back in run queue).
    pub fn block_current(&mut self, cpu_id: u32) {
        self.stats.inc_blocks();
        self.current_threads[cpu_id as usize] = None;
    }
}
