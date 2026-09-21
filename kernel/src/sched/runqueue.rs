//! Unified Hybrid RunQueue for PetraOS.
//!
//! Encapsulates both Real-Time (`RtRunQueue`) and Fair EEVDF (`EevdfScheduler`) queues
//! and the currently executing thread.

use crate::proc::thread::{Thread, ThreadId, ThreadState};
use crate::sched::fair::{BASE_SLICE_NS, EevdfScheduler};
use crate::sched::nice::NICE_0_WEIGHT;
use crate::sched::policy::{DEFAULT_RR_QUANTUM_NS, SchedPolicy};
use crate::sched::realtime::RtRunQueue;
use crate::sync::Mutex;
use alloc::sync::Arc;

/// Unified Hybrid RunQueue managing Real-Time and Fair (EEVDF) tasks.
pub struct RunQueue {
    /// Currently executing thread.
    current: Option<Arc<Mutex<Thread>>>,
    /// Real-time scheduling class (FIFO, Round-Robin).
    rt: RtRunQueue,
    /// Fair scheduling class (EEVDF).
    fair: EevdfScheduler,
}

impl RunQueue {
    /// Creates a new, empty `RunQueue`.
    pub const fn new() -> Self {
        Self {
            current: None,
            rt: RtRunQueue::new(),
            fair: EevdfScheduler::new(),
        }
    }

    /// Returns a clone of the currently executing thread.
    pub fn current(&self) -> Option<Arc<Mutex<Thread>>> {
        self.current.clone()
    }

    /// Sets the currently executing thread.
    pub fn set_current(&mut self, thread: Option<Arc<Mutex<Thread>>>) {
        self.current = thread;
    }

    /// Enqueues a thread into the appropriate queue based on its policy.
    pub fn enqueue(&mut self, thread: Arc<Mutex<Thread>>) {
        let (is_rt, rt_prio) = {
            let t = thread.lock();
            (t.sched_policy.is_realtime(), t.rt_priority)
        };
        if is_rt {
            self.rt.enqueue(thread, rt_prio);
        } else {
            self.fair.enqueue(thread);
        }
    }

    /// Removes a thread by `ThreadId` from any active scheduling queue.
    pub fn dequeue(&mut self, tid: ThreadId) -> Option<Arc<Mutex<Thread>>> {
        self.rt.dequeue(tid).or_else(|| self.fair.dequeue(tid))
    }

    /// Picks the next runnable thread according to the scheduling hierarchy:
    /// 1. Real-Time tasks strictly preempt Fair tasks.
    /// 2. Fair (EEVDF) tasks run when no Real-Time tasks are runnable.
    pub fn pick_next(&mut self) -> Option<Arc<Mutex<Thread>>> {
        let next = self.rt.pick_next().or_else(|| self.fair.pick_next())?;
        next.lock().state = ThreadState::Running;
        Some(next)
    }

    /// Updates scheduling accounting for the currently running thread.
    /// Returns `true` if preemption should trigger.
    pub fn tick(&mut self, delta_ns: u64) -> bool {
        let Some(ref thread) = self.current.clone() else {
            return false;
        };
        let mut t = thread.lock();
        match t.sched_policy {
            SchedPolicy::RoundRobin | SchedPolicy::Fifo => self.rt.update_current(&mut t, delta_ns),
            SchedPolicy::Fair => self.fair.update_current(&mut t, delta_ns),
        }
    }

    /// Voluntarily yields the current thread back into its run queue.
    pub fn yield_current(&mut self) {
        let Some(thread) = self.current.take() else {
            return;
        };
        let policy = thread.lock().sched_policy;
        match policy {
            SchedPolicy::Fair => {
                let (weight, slice_ns) = {
                    let t = thread.lock();
                    let w = if t.weight > 0 {
                        t.weight
                    } else {
                        NICE_0_WEIGHT
                    };
                    let s = if t.slice_ns > 0 {
                        t.slice_ns
                    } else {
                        BASE_SLICE_NS
                    };
                    (w, s)
                };
                let vslice = (slice_ns * NICE_0_WEIGHT as u64) / weight as u64;
                let min_vr = self.fair.min_vruntime;
                {
                    let mut t = thread.lock();
                    t.vruntime = t.vruntime.max(min_vr).saturating_add(vslice);
                    t.vdeadline = t.vruntime.saturating_add(vslice);
                    t.state = ThreadState::Ready;
                }
                self.fair.enqueue(thread);
            }
            SchedPolicy::RoundRobin => {
                let rt_prio = {
                    let mut t = thread.lock();
                    t.rr_remaining_ns = DEFAULT_RR_QUANTUM_NS;
                    t.state = ThreadState::Ready;
                    t.rt_priority
                };
                self.rt.enqueue(thread, rt_prio);
            }
            SchedPolicy::Fifo => {
                let rt_prio = {
                    let mut t = thread.lock();
                    t.state = ThreadState::Ready;
                    t.rt_priority
                };
                self.rt.enqueue(thread, rt_prio);
            }
        }
    }

    /// Total number of queued runnable threads across all queues.
    pub fn len(&self) -> usize {
        self.rt.len() + self.fair.len()
    }

    /// Returns `true` if all scheduling queues are empty.
    pub fn is_empty(&self) -> bool {
        self.rt.is_empty() && self.fair.is_empty()
    }
}
