// SPDX-License-Identifier: GPL-2.0

//! Task scheduling statistics.

/// Scheduling statistics snapshot of a single task.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskStats {
    /// Nice value (-20 to 19).
    pub nice: i8,
    /// Task load weight.
    pub weight: u32,
    /// Requested time slice in nanoseconds.
    pub slice_ns: u64,
    /// Virtual runtime in virtual nanoseconds.
    pub vruntime: u64,
    /// Virtual deadline in virtual nanoseconds.
    pub deadline: u64,
    /// Total execution time in nanoseconds.
    pub exec_ns: u64,
    /// Total wait time in runqueues in nanoseconds.
    pub rq_wait_ns: u64,
    /// Total number of context switches.
    pub nr_switches: u64,
    /// Number of inter-CPU migrations.
    pub nr_migrations: u64,
}

impl core::fmt::Display for TaskStats {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "nice {:3} weight {:5} slice {:>9} vruntime {:>13} deadline {:>13} \
             exec {:>13} rq_wait {:>13} switches {:8} migrations {:5}",
            self.nice,
            self.weight,
            self.slice_ns,
            self.vruntime,
            self.deadline,
            self.exec_ns,
            self.rq_wait_ns,
            self.nr_switches,
            self.nr_migrations,
        )
    }
}
