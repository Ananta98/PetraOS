//! Per-CPU Run Queue implementation for PetraOS.
//!
//! Encapsulates the scheduling classes (Real-Time, Fair/EEVDF) and the currently
//! executing thread for a single CPU core, completely eliminating cross-core
//! scheduling lock contention.

use super::fair::{BASE_SLICE_NS, FairClassRq};
use super::nice::NICE_0_WEIGHT;
use super::policy::{DEFAULT_RR_QUANTUM_NS, SchedPolicy};
use super::realtime::RtClassRq;
use crate::proc::thread::{Thread, ThreadId, ThreadState};
use crate::sync::Mutex;
use alloc::sync::Arc;

/// The per-CPU run queue structure.
///
/// Manages the hierarchy of scheduling classes on a specific CPU:
/// 1. Real-Time (`RtClassRq`): `SCHED_FIFO` and `SCHED_RR`
/// 2. Fair (`FairClassRq`): `SCHED_OTHER` / EEVDF
pub struct PerCpuRunQueue {
    /// CPU identifier for this run queue.
    pub cpu_id: u32,
    /// Currently executing thread on this CPU core.
    pub current: Option<Arc<Mutex<Thread>>>,
    /// Real-time scheduling class.
    pub rt: RtClassRq,
    /// Fair (EEVDF) scheduling class.
    pub fair: FairClassRq,
}

impl PerCpuRunQueue {
    /// Creates a new, empty `PerCpuRunQueue` for `cpu_id`.
    pub fn new(cpu_id: u32) -> Self {
        Self {
            cpu_id,
            current: None,
            rt: RtClassRq::new(),
            fair: FairClassRq::new(),
        }
    }

    /// Obtains a reference to the currently executing thread.
    pub fn current(&self) -> Option<Arc<Mutex<Thread>>> {
        self.current.clone()
    }

    /// Sets the currently executing thread.
    pub fn set_current(&mut self, thread: Option<Arc<Mutex<Thread>>>) {
        self.current = thread;
    }

    /// Enqueues a thread into the appropriate scheduling class based on its policy.
    pub fn enqueue(&mut self, thread: Arc<Mutex<Thread>>) {
        let is_rt = {
            let t = thread.lock();
            t.sched_policy.is_realtime()
        };

        if is_rt {
            self.rt.enqueue(thread);
        } else {
            self.fair.enqueue(thread);
        }
    }

    /// Removes a thread from this CPU's run queues by its `ThreadId`.
    pub fn dequeue(&mut self, tid: ThreadId) -> Option<Arc<Mutex<Thread>>> {
        self.rt.dequeue(tid).or_else(|| self.fair.dequeue(tid))
    }

    /// Picks the next runnable thread according to the scheduling hierarchy:
    /// 1. Real-Time (FIFO / RR) strictly preempts Fair.
    /// 2. Fair (EEVDF) runs when no real-time threads are available.
    pub fn pick_next(&mut self) -> Option<Arc<Mutex<Thread>>> {
        let next = self.rt.pick_next().or_else(|| self.fair.pick_next());
        if let Some(ref thread) = next {
            let mut t = thread.lock();
            t.state = ThreadState::Running;
        }
        next
    }

    /// Updates scheduling accounting for the currently running thread.
    ///
    /// Returns `true` if a preemption should be triggered (e.g. quantum expired).
    pub fn tick(&mut self, delta_ns: u64) -> bool {
        if let Some(ref thread) = self.current {
            let mut t = thread.lock();
            match t.sched_policy {
                SchedPolicy::RoundRobin | SchedPolicy::Fifo => {
                    self.rt.update_current(&mut t, delta_ns)
                }
                SchedPolicy::Fair => {
                    self.fair.update_current(&mut t, delta_ns)
                }
            }
        } else {
            false
        }
    }

    /// Voluntarily yields the currently running thread, placing it back into its run queue.
    pub fn yield_current(&mut self) {
        if let Some(thread) = self.current.take() {
            let mut t_lock = thread.lock();
            let policy = t_lock.sched_policy;

            match policy {
                SchedPolicy::Fair => {
                    let weight = if t_lock.weight > 0 {
                        t_lock.weight
                    } else {
                        NICE_0_WEIGHT
                    };
                    let slice_ns = if t_lock.slice_ns > 0 {
                        t_lock.slice_ns
                    } else {
                        BASE_SLICE_NS
                    };
                    let vslice = (slice_ns * NICE_0_WEIGHT as u64) / weight as u64;

                    let min_vr = self.fair.scheduler.min_vruntime;
                    t_lock.vruntime = t_lock.vruntime.max(min_vr).saturating_add(vslice);
                    t_lock.vdeadline = t_lock.vruntime.saturating_add(vslice);
                    t_lock.state = ThreadState::Ready;
                    drop(t_lock);

                    self.fair.enqueue(thread);
                }
                SchedPolicy::RoundRobin => {
                    t_lock.rr_remaining_ns = DEFAULT_RR_QUANTUM_NS;
                    t_lock.state = ThreadState::Ready;
                    drop(t_lock);

                    self.rt.enqueue(thread);
                }
                SchedPolicy::Fifo => {
                    t_lock.state = ThreadState::Ready;
                    drop(t_lock);

                    self.rt.enqueue(thread);
                }
            }
        }
    }

    /// Total number of queued runnable threads across all scheduling classes.
    pub fn len(&self) -> usize {
        self.rt.len() + self.fair.len()
    }

    /// Returns `true` if all scheduling classes on this CPU core are empty.
    pub fn is_empty(&self) -> bool {
        self.rt.is_empty() && self.fair.is_empty()
    }
}
