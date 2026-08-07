//! Real-Time (RT) scheduler run queue.
//!
//! Implements both **SCHED_FIFO** and **SCHED_RR** (Round-Robin) policies via
//! a fixed-priority array of [`VecDeque<SchedThread>`].
//!
//! # Priority model
//!
//! RT priorities follow POSIX conventions: `1` is the lowest and `99` is the
//! highest. The scheduler always picks the non-empty queue with the highest
//! index (priority).
//!
//! # FIFO vs. Round-Robin
//!
//! * **FIFO** threads run until they block or yield; no time-slice preemption.
//! * **RR** threads share the CPU with peers of the same priority via
//!   [`RtRunQueue::tick`]. When the remaining slice reaches zero the thread is
//!   moved to the back of its priority deque and a new slice is granted.

extern crate alloc;

use alloc::collections::VecDeque;

use crate::sched::sched_thread::{SchedPolicy, SchedThread, ThreadId};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Number of supported RT priority levels (POSIX: 1–99, stored at index 1–99).
const RT_PRIORITIES: usize = 100;

// ── RT run queue ─────────────────────────────────────────────────────────────

/// A real-time run queue for a single CPU.
///
/// Internally backed by a fixed-size array of [`VecDeque`]s, one per priority
/// level. This gives O(1) enqueue / dequeue for the common case where there
/// are only a handful of RT threads.
pub struct RtRunQueue {
    /// `queues[p]` holds threads at RT priority `p`.
    /// Index 0 is unused (RT priority is 1-based).
    queues: [VecDeque<SchedThread>; RT_PRIORITIES],
    /// Total number of threads across all priority levels.
    count: usize,
}

impl RtRunQueue {
    /// Create an empty RT run queue.
    pub fn new() -> Self {
        // SAFETY: VecDeque implements Default; array initialisation is safe.
        Self {
            queues: core::array::from_fn(|_| VecDeque::new()),
            count: 0,
        }
    }

    // ── Capacity ─────────────────────────────────────────────────────────────

    /// Total number of runnable RT threads.
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns `true` if no RT threads are runnable.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    // ── Enqueueing ────────────────────────────────────────────────────────────

    /// Add a thread to the back of its priority deque.
    ///
    /// `thread.priority` must be in `[1, 99]`; values outside this range are
    /// clamped silently.
    pub fn enqueue(&mut self, thread: SchedThread) {
        let prio = thread.priority.clamp(1, 99) as usize;
        self.queues[prio].push_back(thread);
        self.count += 1;
    }

    // ── Dequeueing ────────────────────────────────────────────────────────────

    /// Remove and return the highest-priority runnable thread, breaking ties in
    /// FIFO order within the same priority level.
    ///
    /// Returns `None` if the run queue is empty.
    pub fn dequeue_next(&mut self) -> Option<SchedThread> {
        for prio in (1..RT_PRIORITIES).rev() {
            if let Some(thread) = self.queues[prio].pop_front() {
                self.count -= 1;
                return Some(thread);
            }
        }
        None
    }

    // ── Peeking ───────────────────────────────────────────────────────────────

    /// Return a reference to the next thread that would be dequeued without
    /// removing it, or `None` if empty.
    pub fn pick_next(&self) -> Option<&SchedThread> {
        for prio in (1..RT_PRIORITIES).rev() {
            if let Some(thread) = self.queues[prio].front() {
                return Some(thread);
            }
        }
        None
    }

    // ── Round-Robin tick ─────────────────────────────────────────────────────

    /// Consume `delta_ns` nanoseconds from the currently running RT thread's
    /// time slice (identified by `id`).
    ///
    /// If the thread is a **RoundRobin** thread and its slice expires:
    /// 1. The thread is removed from the front of its priority deque.
    /// 2. Its slice is reset to `thread.time_slice_ns`.
    /// 3. It is re-inserted at the **back** of the same deque.
    ///
    /// Returns `true` if the thread was found, `false` otherwise.
    pub fn tick(&mut self, id: ThreadId, delta_ns: u64) -> bool {
        // Find the priority level of the thread.
        for prio in (1..RT_PRIORITIES).rev() {
            if let Some(front) = self.queues[prio].front_mut() {
                if front.id != id {
                    continue;
                }
                if front.policy != SchedPolicy::RoundRobin {
                    // FIFO threads have no time-slice accounting.
                    return true;
                }

                front.remaining_slice = front.remaining_slice.saturating_sub(delta_ns);

                if front.remaining_slice == 0 {
                    // Rotate to the back of the priority level.
                    let mut thread = self.queues[prio]
                        .pop_front()
                        .expect("front just verified; cannot be None");
                    thread.remaining_slice = thread.time_slice_ns;
                    self.queues[prio].push_back(thread);
                }
                return true;
            }
        }
        false
    }

    // ── Removal ───────────────────────────────────────────────────────────────

    /// Remove a thread by `id` from any priority level.
    ///
    /// Returns the removed thread if found.
    pub fn remove(&mut self, id: ThreadId) -> Option<SchedThread> {
        for prio in (1..RT_PRIORITIES).rev() {
            if let Some(pos) = self.queues[prio].iter().position(|t| t.id == id) {
                let thread = self.queues[prio].remove(pos)?;
                self.count -= 1;
                return Some(thread);
            }
        }
        None
    }
}
