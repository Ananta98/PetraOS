//! Real-Time (RT) Run Queue with O(1) priority selection.
//!
//! Per-priority FIFO queues stored as `VecDeque<Arc<Mutex<Thread>>>`, one slot per
//! priority level. A `BitArray` from `bitvec` gives O(1) highest-priority lookup via `last_one`.

use super::policy::{RT_PRIO_COUNT, RtPriority};
use crate::proc::thread::{Thread, ThreadId};
use crate::sched::policy::{DEFAULT_RR_QUANTUM_NS, SchedPolicy};
use crate::sync::Mutex;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use bitvec::prelude::*;

/// Real-Time Run Queue for a single CPU core.
pub struct RtRunQueue {
    /// Bitmap tracking non-empty priority levels (0..99).
    bitmap: BitArr!(for RT_PRIO_COUNT, in usize, Lsb0),

    /// Per-priority FIFO queues.
    queues: Vec<VecDeque<Arc<Mutex<Thread>>>>,

    /// Total number of queued real-time threads.
    count: usize,
}

impl RtRunQueue {
    /// Creates a new, empty `RtRunQueue`.
    pub fn new() -> Self {
        let mut queues = Vec::with_capacity(RT_PRIO_COUNT);
        for _ in 0..RT_PRIO_COUNT {
            queues.push(VecDeque::new());
        }
        Self {
            bitmap: bitarr![usize, Lsb0; 0; RT_PRIO_COUNT],
            queues,
            count: 0,
        }
    }

    // ── Public API ─────────────────────────────────────────────────────────

    /// Returns the highest non-empty priority level in O(1), or `None` when empty.
    pub fn highest_priority(&self) -> Option<u8> {
        // Higher numerical value = higher RT priority (0..=99).
        self.bitmap.last_one().map(|prio| prio as u8)
    }

    /// Enqueues `thread` at its RT priority level (FIFO within the level).
    pub fn enqueue(&mut self, thread: Arc<Mutex<Thread>>, priority: RtPriority) {
        let prio = priority.value() as usize;
        if prio < RT_PRIO_COUNT {
            self.queues[prio].push_back(thread);
            self.bitmap.set(prio, true);
            self.count += 1;
        }
    }

    /// Removes a thread by `ThreadId` across all priority levels.
    pub fn dequeue(&mut self, tid: ThreadId) -> Option<Arc<Mutex<Thread>>> {
        for prio in self.bitmap.iter_ones().rev() {
            let q = &mut self.queues[prio];
            if let Some(idx) = q.iter().position(|t| t.lock().tid == tid) {
                let thread = q.remove(idx)?;
                if q.is_empty() {
                    self.bitmap.set(prio, false);
                }
                self.count = self.count.saturating_sub(1);
                return Some(thread);
            }
        }
        None
    }

    /// Pops the highest-priority thread (front of its FIFO queue).
    pub fn pick_next(&mut self) -> Option<Arc<Mutex<Thread>>> {
        while let Some(prio) = self.highest_priority() {
            let q = &mut self.queues[prio as usize];
            if let Some(thread) = q.pop_front() {
                if q.is_empty() {
                    self.bitmap.set(prio as usize, false);
                }
                self.count = self.count.saturating_sub(1);
                return Some(thread);
            }
            self.bitmap.set(prio as usize, false); // stale bit — clear and retry
        }
        None
    }

    /// Updates RR quantum accounting. Returns `true` when the quantum expires.
    pub fn update_current(&mut self, thread: &mut Thread, delta_ns: u64) -> bool {
        if thread.sched_policy == SchedPolicy::RoundRobin {
            if thread.rr_remaining_ns <= delta_ns {
                thread.rr_remaining_ns = DEFAULT_RR_QUANTUM_NS;
                return true;
            }
            thread.rr_remaining_ns = thread.rr_remaining_ns.saturating_sub(delta_ns);
        }
        false
    }

    /// Returns the number of queued real-time threads.
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns `true` when no real-time threads are queued.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl Default for RtRunQueue {
    fn default() -> Self {
        Self::new()
    }
}
