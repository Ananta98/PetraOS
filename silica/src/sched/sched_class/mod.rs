// SPDX-License-Identifier: GPL-2.0

//! Scheduling classes and per-CPU runqueue components.
//!
//! Organizes scheduling into a hierarchy of scheduling classes:
//! Real-Time (`SCHED_FIFO`, `SCHED_RR`) > Fair (`SCHED_OTHER`, `SCHED_BATCH`, `SCHED_IDLE`) > Idle.

pub mod eevdf;
pub mod rq;
pub mod rt;

pub use eevdf::{EevdfEntity, EevdfRunQueue};
pub use rq::{RunQueue, SchedEntry};
pub use rt::{DEF_RT_TIMESLICE_NS, RtEntity, RtRunQueue};

use crate::api::sched::{CpuMask, SCHED_FIFO, SCHED_IDLE, SCHED_OTHER, SCHED_RR};
use crate::sched::stats::TaskStats;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use ostd::sync::SpinLock;

/// Unified scheduling entity attached to an OSTD task.
pub struct SchedEntity {
    /// Active scheduling policy (`SCHED_OTHER`, `SCHED_FIFO`, `SCHED_RR`, etc.).
    pub policy: AtomicI32,
    /// Real-time scheduling state.
    pub rt: RtEntity,
    /// EEVDF fair scheduling state.
    pub fair: EevdfEntity,
    /// CPU affinity mask.
    pub affinity: SpinLock<CpuMask>,
    /// Total CPU execution time in nanoseconds.
    pub exec_ns: AtomicU64,
    /// Total wait time in runqueues in nanoseconds.
    pub rq_wait_ns: AtomicU64,
    /// Total number of context switches.
    pub nr_switches: AtomicU64,
    /// Number of inter-CPU migrations.
    pub nr_migrations: AtomicU64,
    /// Tick at which the task was last dispatched or charged.
    pub last_run: AtomicU64,
    /// Unique sequence identifier for deterministic tie-breaking.
    pub seq: usize,
}

impl SchedEntity {
    /// Creates a new fair EEVDF scheduling entity with the given nice value and slice.
    pub fn new_fair(nice: i8, slice_ns: u64, seq: usize) -> Arc<Self> {
        Arc::new(Self {
            policy: AtomicI32::new(SCHED_OTHER),
            rt: RtEntity::new(0),
            fair: EevdfEntity::new(nice, slice_ns),
            affinity: SpinLock::new(CpuMask::all()),
            exec_ns: AtomicU64::new(0),
            rq_wait_ns: AtomicU64::new(0),
            nr_switches: AtomicU64::new(0),
            nr_migrations: AtomicU64::new(0),
            last_run: AtomicU64::new(0),
            seq,
        })
    }

    /// Creates a new real-time scheduling entity with the given policy and priority.
    pub fn new_rt(policy: i32, priority: i32, seq: usize) -> Arc<Self> {
        Arc::new(Self {
            policy: AtomicI32::new(policy),
            rt: RtEntity::new(priority),
            fair: EevdfEntity::new(0, eevdf::BASE_SLICE_NS),
            affinity: SpinLock::new(CpuMask::all()),
            exec_ns: AtomicU64::new(0),
            rq_wait_ns: AtomicU64::new(0),
            nr_switches: AtomicU64::new(0),
            nr_migrations: AtomicU64::new(0),
            last_run: AtomicU64::new(0),
            seq,
        })
    }

    /// Creates an idle scheduling entity.
    pub fn idle() -> Arc<Self> {
        Arc::new(Self {
            policy: AtomicI32::new(SCHED_IDLE),
            rt: RtEntity::new(0),
            fair: EevdfEntity::new(0, eevdf::BASE_SLICE_NS),
            affinity: SpinLock::new(CpuMask::all()),
            exec_ns: AtomicU64::new(0),
            rq_wait_ns: AtomicU64::new(0),
            nr_switches: AtomicU64::new(0),
            nr_migrations: AtomicU64::new(0),
            last_run: AtomicU64::new(0),
            seq: 0,
        })
    }

    /// Returns the current scheduling policy.
    pub fn policy(&self) -> i32 {
        self.policy.load(Ordering::Relaxed)
    }

    /// Returns whether this entity is a real-time task.
    pub fn is_rt(&self) -> bool {
        matches!(self.policy(), SCHED_FIFO | SCHED_RR)
    }

    /// Updates CPU affinity mask.
    pub fn set_affinity(&self, mask: CpuMask) {
        *self.affinity.lock() = mask;
    }

    /// Returns the current CPU affinity mask.
    pub fn affinity(&self) -> CpuMask {
        *self.affinity.lock()
    }

    /// Updates policy, priority, and nice parameters.
    pub fn set_params(&self, policy: i32, priority: i32, nice: i8) {
        self.policy.store(policy, Ordering::Relaxed);
        if matches!(policy, SCHED_FIFO | SCHED_RR) {
            self.rt.set_priority(priority);
        } else {
            self.fair.set_nice(nice);
        }
    }

    /// Records that the task was dispatched to a CPU at tick `now` after waiting since `arrived`.
    pub fn dispatched(&self, now: u64, arrived: u64) {
        self.nr_switches.fetch_add(1, Ordering::Relaxed);
        self.last_run.store(now, Ordering::Relaxed);
        let wait_ticks = now.saturating_sub(arrived);
        let wait_ns = wait_ticks.saturating_mul(crate::sched::NSEC_PER_TICK);
        self.rq_wait_ns.fetch_add(wait_ns, Ordering::Relaxed);
    }

    /// Charges real execution time in nanoseconds.
    pub fn charge_exec(&self, delta_ns: u64) {
        self.exec_ns.fetch_add(delta_ns, Ordering::Relaxed);
    }

    /// Takes a consistent snapshot of task statistics.
    pub fn stats(&self) -> TaskStats {
        TaskStats {
            nice: self.fair.nice(),
            weight: self.fair.weight(),
            slice_ns: self.fair.slice_ns(),
            vruntime: self.fair.vruntime(),
            deadline: self.fair.deadline(),
            exec_ns: self.exec_ns.load(Ordering::Relaxed),
            rq_wait_ns: self.rq_wait_ns.load(Ordering::Relaxed),
            nr_switches: self.nr_switches.load(Ordering::Relaxed),
            nr_migrations: self.nr_migrations.load(Ordering::Relaxed),
        }
    }
}
