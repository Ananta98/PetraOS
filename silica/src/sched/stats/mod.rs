// SPDX-License-Identifier: GPL-2.0

//! Scheduler statistics and load monitoring subsystem.

pub mod cpu;
pub mod global;
pub mod loadavg;
pub mod task;

pub use cpu::{CpuStats, CpuStatsSnapshot};
pub use global::{GlobalStats, GlobalStatsSnapshot, global, global_snapshot};
pub use loadavg::{LOAD_AVG_MAX, LOAD_AVG_SCALE, LoadAvg, loadavg, on_tick as on_loadavg_tick};
pub use task::TaskStats;

use ostd::task::Task;

/// Returns the scheduling statistics for a task if tracked by the scheduler.
pub fn for_task(task: &Task) -> Option<TaskStats> {
    crate::sched::entity_of(task).map(|entity| entity.stats())
}
