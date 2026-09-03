//! Real-Time (RT) Run Queue with O(1) priority selection.
//!
//! Provides a priority-indexed run queue supporting 100 distinct real-time priority levels (0..=99).
//! Highest priority selection is O(1) using CPU intrinsic `leading_zeros` (`lzcnt`/`bsr`).

use super::policy::{RT_PRIO_COUNT, RtPriority};
use crate::proc::thread::{Thread, ThreadId};
use crate::sched::policy::{DEFAULT_RR_QUANTUM_NS, SchedPolicy};
use crate::sync::Mutex;
use alloc::collections::VecDeque;
use alloc::sync::Arc;

/// Real-Time Run Queue for a single CPU core.
pub struct RtRunQueue {
    /// Bitmaps tracking non-empty priority levels.
    /// `bitmap[0]`: priorities 0..63
    /// `bitmap[1]`: priorities 64..99
    bitmap: [u64; 2],

    /// Per-priority FIFO queues.
    queues: [VecDeque<Arc<Mutex<Thread>>>; RT_PRIO_COUNT],

    /// Total number of queued real-time threads.
    count: usize,
}

impl RtRunQueue {
    /// Creates a new, empty `RtRunQueue`.
    pub const fn new() -> Self {
        Self {
            bitmap: [0, 0],
            queues: [const { VecDeque::new() }; RT_PRIO_COUNT],
            count: 0,
        }
    }

    /// Sets the bitmap bit for the specified priority level.
    #[inline]
    fn set_bit(&mut self, prio: u8) {
        let (word, bit) = if prio < 64 { (0, prio) } else { (1, prio - 64) };
        self.bitmap[word] |= 1 << bit;
    }

    /// Clears the bitmap bit for the specified priority level.
    #[inline]
    fn clear_bit(&mut self, prio: u8) {
        let (word, bit) = if prio < 64 { (0, prio) } else { (1, prio - 64) };
        self.bitmap[word] &= !(1 << bit);
    }

    /// Finds the highest non-empty priority level in O(1) time.
    /// Returns `None` if the queue is completely empty.
    pub fn find_highest_priority(&self) -> Option<u8> {
        // Check priorities 64..99 first (higher numerical value = higher priority)
        let high_word = self.bitmap[1];
        if high_word != 0 {
            let leading = high_word.leading_zeros() as u8;
            let bit = 63 - leading;
            return Some(64 + bit);
        }

        // Check priorities 0..63
        let low_word = self.bitmap[0];
        if low_word != 0 {
            let leading = low_word.leading_zeros() as u8;
            let bit = 63 - leading;
            return Some(bit);
        }

        None
    }

    /// Enqueues a real-time thread into the appropriate priority queue.
    pub fn enqueue(&mut self, thread: Arc<Mutex<Thread>>, priority: RtPriority) {
        let prio = priority.value();
        if (prio as usize) < RT_PRIO_COUNT {
            self.queues[prio as usize].push_back(thread);
            self.set_bit(prio);
            self.count += 1;
        }
    }

    /// Removes a thread by its `ThreadId` across all priority queues.
    pub fn dequeue(&mut self, tid: ThreadId) -> Option<Arc<Mutex<Thread>>> {
        for prio in 0..RT_PRIO_COUNT as u8 {
            let (word, bit) = if prio < 64 { (0, prio) } else { (1, prio - 64) };
            if (self.bitmap[word] & (1 << bit)) == 0 {
                continue;
            }
            let q = &mut self.queues[prio as usize];
            if let Some(idx) = q.iter().position(|t| t.lock().tid == tid) {
                let thread = q.remove(idx);
                if q.is_empty() {
                    self.clear_bit(prio);
                }
                self.count = self.count.saturating_sub(1);
                return thread;
            }
        }
        None
    }

    /// Pops the next highest-priority real-time thread.
    ///
    /// Returns `None` if there are no runnable real-time threads.
    pub fn dequeue_highest(&mut self) -> Option<Arc<Mutex<Thread>>> {
        while let Some(prio) = self.find_highest_priority() {
            let q = &mut self.queues[prio as usize];
            if let Some(thread) = q.pop_front() {
                if q.is_empty() {
                    self.clear_bit(prio);
                }
                self.count = self.count.saturating_sub(1);
                return Some(thread);
            } else {
                self.clear_bit(prio);
            }
        }
        None
    }

    /// Checks whether the real-time run queue has any runnable threads.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Returns the number of queued real-time threads.
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }
}

/// Real-Time Scheduling Class Run Queue wrapper.
pub struct RtClassRq {
    queue: RtRunQueue,
}

impl RtClassRq {
    /// Creates a new `RtClassRq`.
    pub const fn new() -> Self {
        Self {
            queue: RtRunQueue::new(),
        }
    }

    /// Returns the human-readable name of this scheduling class.
    pub fn name(&self) -> &'static str {
        "RealTime"
    }

    /// Enqueues a runnable thread into the real-time run queue.
    pub fn enqueue(&mut self, thread: Arc<Mutex<Thread>>) {
        let rt_prio = thread.lock().rt_priority;
        self.queue.enqueue(thread, rt_prio);
    }

    /// Removes a thread by its `ThreadId`.
    pub fn dequeue(&mut self, tid: ThreadId) -> Option<Arc<Mutex<Thread>>> {
        self.queue.dequeue(tid)
    }

    /// Picks the next highest-priority real-time thread.
    pub fn pick_next(&mut self) -> Option<Arc<Mutex<Thread>>> {
        self.queue.dequeue_highest()
    }

    /// Returns the number of runnable real-time threads.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Checks if the real-time run queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Updates quantum accounting and returns true if round-robin preemption should be triggered.
    pub fn update_current(&mut self, thread: &mut Thread, delta_ns: u64) -> bool {
        if thread.sched_policy == SchedPolicy::RoundRobin {
            if thread.rr_remaining_ns <= delta_ns {
                thread.rr_remaining_ns = DEFAULT_RR_QUANTUM_NS;
                return true; // Quantum expired, preempt
            } else {
                thread.rr_remaining_ns = thread.rr_remaining_ns.saturating_sub(delta_ns);
            }
        }
        false
    }
}

impl Default for RtClassRq {
    fn default() -> Self {
        Self::new()
    }
}
