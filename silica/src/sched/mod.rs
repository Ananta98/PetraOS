// SPDX-License-Identifier: GPL-2.0

//! Task scheduling subsystem.
//!
//! Implements a per-CPU multi-class scheduler combining Real-Time (`SCHED_FIFO`
//! and `SCHED_RR`) and Fair Earliest Eligible Virtual Deadline First (EEVDF)
//! scheduling classes.
//!
//! # Architecture
//!
//! - `sched_class`: Per-CPU runqueues and class engines:
//!   - `rt`: Strict priority array (`1..=99`) with round-robin quantum rotation.
//!   - `eevdf`: Latency-sensitive proportional share virtual deadline scheduler.
//!   - `rq`: Composite per-CPU runqueue orchestrating class precedence:
//!     `Real-Time > EEVDF > Idle`.
//! - `stats`: System load monitoring (`/proc/loadavg`) and CPU/task statistics.
//! - `adapter`: Glue between OSTD `Scheduler`/`LocalRunQueue` traits and runqueues.
//! - `attrs`: Process and thread scheduling attributes, affinity, and permissions.

pub mod adapter;
pub mod attrs;
pub mod sched_class;
pub mod stats;

pub use attrs::*;
pub use stats::loadavg;

use ostd::timer::{Jiffies, TIMER_FREQ};

pub use self::{
    adapter::{
        build, cpu_stats_on, entity_of, init, init_on_cpu, nr_running_on, set_task_sched_params,
    },
    sched_class::{
        RunQueue, SchedEntity, SchedEntry,
        eevdf::{
            BASE_SLICE_NS, MAX_SLICE_NS, MIN_SLICE_NS, NICE_0_LOAD, NICE_MAX, NICE_MIN,
            weight_from_nice,
        },
        rt::{DEF_RT_TIMESLICE_NS, RtEntity, RtRunQueue},
    },
    stats::{CpuStatsSnapshot, GlobalStatsSnapshot, LoadAvg, TaskStats},
};

/// Backward-compatible alias for task scheduling entity.
pub type Entity = SchedEntity;

/// Number of timer ticks per second.
pub const TIMER_HZ: u64 = TIMER_FREQ;

/// Length of one timer tick in nanoseconds.
pub const NSEC_PER_TICK: u64 = 1_000_000_000 / TIMER_HZ;

/// Returns the elapsed timer ticks since system boot.
pub(crate) fn now() -> u64 {
    Jiffies::elapsed().as_u64()
}
