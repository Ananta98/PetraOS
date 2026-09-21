//! Real-Time (RT) Run Queue with O(1) priority selection.
//!
//! Per-priority FIFO queues stored as fixed array `[VecDeque<(ThreadId, Arc<Mutex<Thread>>)>; RT_PRIO_COUNT]`.
//! A native `u128` bitmap gives O(1) highest-priority lookup via `leading_zeros`.
//! A secondary `BTreeMap<ThreadId, u8>` index provides fast O(log N) dequeue without scanning
//! all 100 priority levels or acquiring thread spinlocks.

use super::policy::{RT_PRIO_COUNT, RtPriority};
use crate::proc::thread::{Thread, ThreadId};
use crate::sched::policy::{DEFAULT_RR_QUANTUM_NS, SchedPolicy};
use crate::sync::Mutex;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;

/// Real-Time Run Queue for a single CPU core.
pub struct RtRunQueue {
    /// Native bitmap tracking non-empty priority levels (0..99).
    bitmap: u128,

    /// Per-priority FIFO queues holding (ThreadId, Arc<Mutex<Thread>>).
    queues: [VecDeque<(ThreadId, Arc<Mutex<Thread>>)>; RT_PRIO_COUNT],

    /// Secondary index mapping ThreadId -> priority level for fast dequeue.
    by_tid: BTreeMap<ThreadId, u8>,

    /// Total number of queued real-time threads.
    count: usize,
}

impl RtRunQueue {
    /// Creates a new, empty `RtRunQueue`.
    pub const fn new() -> Self {
        Self {
            bitmap: 0,
            queues: [const { VecDeque::new() }; RT_PRIO_COUNT],
            by_tid: BTreeMap::new(),
            count: 0,
        }
    }

    // ── Public API ─────────────────────────────────────────────────────────

    /// Returns the highest non-empty priority level in O(1), or `None` when empty.
    #[inline(always)]
    pub fn highest_priority(&self) -> Option<u8> {
        // Higher numerical value = higher RT priority (0..=99).
        if self.bitmap == 0 {
            None
        } else {
            Some((127 - self.bitmap.leading_zeros()) as u8)
        }
    }

    /// Enqueues `thread` at its RT priority level (FIFO within the level).
    pub fn enqueue(&mut self, thread: Arc<Mutex<Thread>>, priority: RtPriority) {
        let prio = priority.value() as usize;
        if prio < RT_PRIO_COUNT {
            let tid = thread.lock().tid;
            self.queues[prio].push_back((tid, thread));
            self.by_tid.insert(tid, prio as u8);
            self.bitmap |= 1u128 << prio;
            self.count += 1;
        }
    }

    /// Removes a thread by `ThreadId` in O(log N) without scanning or locking other threads.
    pub fn dequeue(&mut self, tid: ThreadId) -> Option<Arc<Mutex<Thread>>> {
        let prio = self.by_tid.remove(&tid)? as usize;
        let q = &mut self.queues[prio];
        let idx = q.iter().position(|entry| entry.0 == tid)?;
        let (_, thread) = q.remove(idx)?;
        if q.is_empty() {
            self.bitmap &= !(1u128 << prio);
        }
        self.count = self.count.saturating_sub(1);
        Some(thread)
    }

    /// Pops the highest-priority thread in O(1).
    pub fn pick_next(&mut self) -> Option<Arc<Mutex<Thread>>> {
        while let Some(prio) = self.highest_priority() {
            let q = &mut self.queues[prio as usize];
            if let Some((tid, thread)) = q.pop_front() {
                self.by_tid.remove(&tid);
                if q.is_empty() {
                    self.bitmap &= !(1u128 << prio);
                }
                self.count = self.count.saturating_sub(1);
                return Some(thread);
            }
            self.bitmap &= !(1u128 << prio); // stale bit — clear and retry
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
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns `true` when no real-time threads are queued.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl Default for RtRunQueue {
    fn default() -> Self {
        Self::new()
    }
}
