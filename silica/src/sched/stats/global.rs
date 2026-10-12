// SPDX-License-Identifier: GPL-2.0

//! System-wide scheduling statistics.

use core::sync::atomic::{AtomicU64, Ordering};

/// System-wide global scheduling statistics.
#[derive(Debug, Default)]
pub struct GlobalStats {
    ctxt: AtomicU64,
    nr_ticks: AtomicU64,
    nr_entities: AtomicU64,
}

impl GlobalStats {
    /// Creates a zeroed set of global counters.
    pub const fn new() -> Self {
        Self {
            ctxt: AtomicU64::new(0),
            nr_ticks: AtomicU64::new(0),
            nr_entities: AtomicU64::new(0),
        }
    }

    /// Records a system-wide context switch.
    pub fn switched(&self) {
        self.ctxt.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a system timer tick.
    pub fn ticked(&self) {
        self.nr_ticks.fetch_add(1, Ordering::Relaxed);
    }

    /// Records the creation of a scheduling entity.
    pub fn entity_created(&self) {
        self.nr_entities.fetch_add(1, Ordering::Relaxed);
    }

    /// Takes a consistent snapshot of the system-wide counters.
    pub fn snapshot(&self) -> GlobalStatsSnapshot {
        GlobalStatsSnapshot {
            ctxt: self.ctxt.load(Ordering::Relaxed),
            nr_ticks: self.nr_ticks.load(Ordering::Relaxed),
            nr_entities: self.nr_entities.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of system-wide scheduling counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlobalStatsSnapshot {
    pub ctxt: u64,
    pub nr_ticks: u64,
    pub nr_entities: u64,
}

impl core::fmt::Display for GlobalStatsSnapshot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "ctxt {:10} ticks {:10} tracked tasks {:5}",
            self.ctxt, self.nr_ticks, self.nr_entities
        )
    }
}

static GLOBAL: GlobalStats = GlobalStats::new();

/// Returns a reference to the global scheduler counters.
pub fn global() -> &'static GlobalStats {
    &GLOBAL
}

/// Returns a snapshot of the global scheduler counters.
pub fn global_snapshot() -> GlobalStatsSnapshot {
    GLOBAL.snapshot()
}
