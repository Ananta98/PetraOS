// SPDX-License-Identifier: GPL-2.0

//! Per-CPU scheduling statistics.

use core::sync::atomic::{AtomicU64, Ordering};

/// Cumulative scheduling statistics for a single CPU.
#[derive(Debug, Default)]
pub struct CpuStats {
    nr_switches: AtomicU64,
    nr_voluntary_switches: AtomicU64,
    nr_involuntary_switches: AtomicU64,
    nr_migrations: AtomicU64,
    exec_ns: AtomicU64,
    idle_ns: AtomicU64,
}

impl CpuStats {
    /// Creates a zeroed set of CPU statistics counters.
    pub const fn new() -> Self {
        Self {
            nr_switches: AtomicU64::new(0),
            nr_voluntary_switches: AtomicU64::new(0),
            nr_involuntary_switches: AtomicU64::new(0),
            nr_migrations: AtomicU64::new(0),
            exec_ns: AtomicU64::new(0),
            idle_ns: AtomicU64::new(0),
        }
    }

    /// Records execution time on this CPU.
    pub fn ran(&self, delta_ns: u64) {
        self.exec_ns.fetch_add(delta_ns, Ordering::Relaxed);
    }

    /// Records idle time on this CPU.
    pub fn idled(&self, delta_ns: u64) {
        self.idle_ns.fetch_add(delta_ns, Ordering::Relaxed);
    }

    /// Records a context switch.
    pub fn switched_out(&self, voluntary: bool) {
        self.nr_switches.fetch_add(1, Ordering::Relaxed);
        if voluntary {
            self.nr_voluntary_switches.fetch_add(1, Ordering::Relaxed);
        } else {
            self.nr_involuntary_switches.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Takes a consistent snapshot of the CPU statistics counters.
    pub fn snapshot(&self) -> CpuStatsSnapshot {
        CpuStatsSnapshot {
            nr_switches: self.nr_switches.load(Ordering::Relaxed),
            nr_voluntary_switches: self.nr_voluntary_switches.load(Ordering::Relaxed),
            nr_involuntary_switches: self.nr_involuntary_switches.load(Ordering::Relaxed),
            nr_migrations: self.nr_migrations.load(Ordering::Relaxed),
            exec_ns: self.exec_ns.load(Ordering::Relaxed),
            idle_ns: self.idle_ns.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of a CPU's scheduling counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuStatsSnapshot {
    pub nr_switches: u64,
    pub nr_voluntary_switches: u64,
    pub nr_involuntary_switches: u64,
    pub nr_migrations: u64,
    pub exec_ns: u64,
    pub idle_ns: u64,
}

impl core::fmt::Display for CpuStatsSnapshot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "switches {:9} (voluntary {:9}, involuntary {:9}) \
             migrations {:5} exec {:13} idle {:13}",
            self.nr_switches,
            self.nr_voluntary_switches,
            self.nr_involuntary_switches,
            self.nr_migrations,
            self.exec_ns,
            self.idle_ns,
        )
    }
}
