// SPDX-License-Identifier: GPL-2.0

//! Real-time scheduling class (SCHED_FIFO and SCHED_RR).
//!
//! Enforces strict priority-based scheduling across 99 real-time priority levels (1..=99).
//! Higher priority numbers strictly preempt lower priority tasks.
//! `SCHED_FIFO` tasks run until blocked or yielded; `SCHED_RR` tasks share CPU time
//! within the same priority level using a 100 ms round-robin time quantum.

use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use ostd::task::Task;

use super::rq::SchedEntry;
use crate::uapi::sched::{RT_PRIO_MAX, SCHED_RR};

/// Default round-robin time quantum (100 ms).
pub const DEF_RT_TIMESLICE_NS: u64 = 100_000_000;

/// Real-time scheduling parameters of a task.
pub struct RtEntity {
    priority: AtomicI32,
    time_slice_ns: AtomicU64,
}

impl RtEntity {
    /// Creates a new real-time entity with the specified priority.
    pub fn new(priority: i32) -> Self {
        Self {
            priority: AtomicI32::new(priority.clamp(0, RT_PRIO_MAX)),
            time_slice_ns: AtomicU64::new(DEF_RT_TIMESLICE_NS),
        }
    }

    /// Returns the real-time priority (1..=99).
    pub fn priority(&self) -> i32 {
        self.priority.load(Ordering::Relaxed)
    }

    /// Sets the real-time priority.
    pub fn set_priority(&self, prio: i32) {
        self.priority
            .store(prio.clamp(0, RT_PRIO_MAX), Ordering::Relaxed);
    }

    /// Returns the remaining round-robin quantum in nanoseconds.
    pub fn time_slice_ns(&self) -> u64 {
        self.time_slice_ns.load(Ordering::Relaxed)
    }

    /// Resets the round-robin quantum to the default 100 ms.
    pub fn reset_time_slice(&self) {
        self.time_slice_ns
            .store(DEF_RT_TIMESLICE_NS, Ordering::Relaxed);
    }

    /// Charges quantum time for `SCHED_RR`. Returns true if the quantum expired.
    pub fn charge_slice(&self, delta_ns: u64) -> bool {
        let current = self.time_slice_ns.load(Ordering::Relaxed);
        let updated = current.saturating_sub(delta_ns);
        self.time_slice_ns.store(updated, Ordering::Relaxed);
        updated == 0
    }
}

/// Real-time runqueue for a single CPU.
pub struct RtRunQueue {
    /// Priority queues indexed by priority 0..=99 (index 0 unused/idle).
    prio_queues: [VecDeque<SchedEntry>; 100],
    /// 128-bit bitmap indicating non-empty priority levels.
    prio_bitmap: u128,
    /// Total number of runnable real-time tasks.
    nr_running: usize,
}

impl Default for RtRunQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl RtRunQueue {
    /// Creates an empty real-time runqueue.
    pub fn new() -> Self {
        // Initialize 100 empty VecDeques
        const EMPTY_DEQUE: VecDeque<SchedEntry> = VecDeque::new();
        Self {
            prio_queues: [EMPTY_DEQUE; 100],
            prio_bitmap: 0,
            nr_running: 0,
        }
    }

    /// Returns the number of runnable real-time tasks.
    pub fn nr_running(&self) -> usize {
        self.nr_running
    }

    /// Returns true if any real-time tasks are queued.
    pub fn has_tasks(&self) -> bool {
        self.nr_running > 0
    }

    /// Returns the highest active real-time priority, if any.
    pub fn highest_prio(&self) -> Option<i32> {
        if self.prio_bitmap == 0 {
            None
        } else {
            // Find highest set bit (0..=99)
            let highest = 127 - self.prio_bitmap.leading_zeros() as usize;
            Some(highest as i32)
        }
    }

    /// Enqueues a real-time task.
    pub fn enqueue(&mut self, entry: SchedEntry, at_head: bool) {
        let prio = entry.entity.rt.priority().clamp(1, 99) as usize;
        if at_head {
            self.prio_queues[prio].push_front(entry);
        } else {
            self.prio_queues[prio].push_back(entry);
        }
        self.prio_bitmap |= 1u128 << prio;
        self.nr_running += 1;
    }

    /// Dequeues a task matching the given `Task` pointer.
    pub fn dequeue(&mut self, task: &Task) -> Option<SchedEntry> {
        if self.nr_running == 0 {
            return None;
        }
        let task_ptr = task as *const Task;
        for prio in (1..=99).rev() {
            if (self.prio_bitmap & (1u128 << prio)) == 0 {
                continue;
            }
            if let Some(pos) = self.prio_queues[prio]
                .iter()
                .position(|e| core::ptr::eq(&*e.task, task_ptr))
            {
                let entry = self.prio_queues[prio].remove(pos)?;
                if self.prio_queues[prio].is_empty() {
                    self.prio_bitmap &= !(1u128 << prio);
                }
                self.nr_running -= 1;
                return Some(entry);
            }
        }
        None
    }

    /// Returns a reference to the highest-priority runnable real-time task.
    pub fn pick_next(&self) -> Option<&SchedEntry> {
        let highest = self.highest_prio()? as usize;
        self.prio_queues[highest].front()
    }

    /// Handles a timer tick for the running real-time task.
    ///
    /// Returns true if a reschedule/preemption is requested.
    pub fn tick(&mut self, current: &SchedEntry, delta_ns: u64) -> bool {
        let prio = current.entity.rt.priority().clamp(1, 99) as usize;

        // If a higher priority RT task arrived, preempt immediately.
        if let Some(highest) = self.highest_prio()
            && highest > prio as i32
        {
            return true;
        }

        // If running under SCHED_RR, charge quantum.
        if current.entity.policy() == SCHED_RR {
            let expired = current.entity.rt.charge_slice(delta_ns);
            if expired {
                // If there are peer tasks at the same priority level, yield CPU.
                if !self.prio_queues[prio].is_empty() {
                    return true;
                } else {
                    current.entity.rt.reset_time_slice();
                }
            }
        }

        false
    }

    /// Rotates the running real-time task to the back of its priority queue upon yield.
    pub fn yield_task(&mut self, current: &SchedEntry) {
        let prio = current.entity.rt.priority().clamp(1, 99) as usize;
        // Re-enqueue at the back of the queue
        self.prio_queues[prio].push_back(current.clone());
        self.prio_bitmap |= 1u128 << prio;
        self.nr_running += 1;
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_rt_entity_prio_and_slice() {
        let rt = RtEntity::new(50);
        assert_eq!(rt.priority(), 50);
        assert_eq!(rt.time_slice_ns(), DEF_RT_TIMESLICE_NS);

        // Charge 50 ms
        let expired = rt.charge_slice(50_000_000);
        assert!(!expired);
        assert_eq!(rt.time_slice_ns(), 50_000_000);

        // Charge another 50 ms -> expires
        let expired = rt.charge_slice(50_000_000);
        assert!(expired);
        assert_eq!(rt.time_slice_ns(), 0);

        // Reset
        rt.reset_time_slice();
        assert_eq!(rt.time_slice_ns(), DEF_RT_TIMESLICE_NS);
    }

    #[ktest]
    fn test_rt_runqueue_highest_prio() {
        let mut rq = RtRunQueue::new();
        assert_eq!(rq.highest_prio(), None);
        assert!(!rq.has_tasks());

        // Set bits directly in bitmap for testing priority calculation
        rq.prio_bitmap |= 1u128 << 10;
        rq.nr_running = 1;
        assert_eq!(rq.highest_prio(), Some(10));

        rq.prio_bitmap |= 1u128 << 80;
        rq.nr_running = 2;
        assert_eq!(rq.highest_prio(), Some(80));

        rq.prio_bitmap |= 1u128 << 99;
        rq.nr_running = 3;
        assert_eq!(rq.highest_prio(), Some(99));
    }

    #[ktest]
    fn test_rt_bitmap_full_spectrum() {
        let mut rq = RtRunQueue::new();
        // Priority 1 (lowest valid RT priority)
        rq.prio_bitmap |= 1u128 << 1;
        assert_eq!(rq.highest_prio(), Some(1));

        // Priority 64 (spanning across 64-bit boundary)
        rq.prio_bitmap |= 1u128 << 64;
        assert_eq!(rq.highest_prio(), Some(64));

        // Priority 99 (highest valid RT priority)
        rq.prio_bitmap |= 1u128 << 99;
        assert_eq!(rq.highest_prio(), Some(99));

        // Clear priority 99
        rq.prio_bitmap &= !(1u128 << 99);
        assert_eq!(rq.highest_prio(), Some(64));

        // Clear priority 64
        rq.prio_bitmap &= !(1u128 << 64);
        assert_eq!(rq.highest_prio(), Some(1));

        // Clear priority 1
        rq.prio_bitmap &= !(1u128 << 1);
        assert_eq!(rq.highest_prio(), None);
    }

    #[ktest]
    fn test_rt_priority_clamping() {
        let rt_low = RtEntity::new(-10);
        assert_eq!(rt_low.priority(), 0);

        let rt_high = RtEntity::new(150);
        assert_eq!(rt_high.priority(), RT_PRIO_MAX);

        rt_low.set_priority(99);
        assert_eq!(rt_low.priority(), 99);

        rt_low.set_priority(200);
        assert_eq!(rt_low.priority(), RT_PRIO_MAX);
    }

    #[ktest]
    fn test_rt_slice_charge_boundaries() {
        let rt = RtEntity::new(20);
        assert_eq!(rt.time_slice_ns(), 100_000_000);

        // 10 charges of 10 ms each
        for i in 1..=9 {
            assert!(!rt.charge_slice(10_000_000));
            assert_eq!(rt.time_slice_ns(), 100_000_000 - i * 10_000_000);
        }
        // 10th charge hits 0
        assert!(rt.charge_slice(10_000_000));
        assert_eq!(rt.time_slice_ns(), 0);

        // Saturating sub beyond 0 remains 0
        assert!(rt.charge_slice(10_000_000));
        assert_eq!(rt.time_slice_ns(), 0);
    }
}
