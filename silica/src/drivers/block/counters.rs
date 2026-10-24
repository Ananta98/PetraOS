// SPDX-License-Identifier: GPL-2.0

//! Shared Block Device I/O Counters.
//!
//! Provides atomic operation counters tracking completed I/O requests and sector counts,
//! mapping directly into Linux `/proc/diskstats` metrics ([`BlockDeviceStats`]).

use core::sync::atomic::{AtomicU64, Ordering};
use crate::drivers::BlockDeviceStats;

/// Thread-safe block device I/O counters backing [`BlockDevice::stats`](crate::drivers::BlockDevice::stats).
#[derive(Default)]
pub struct BlockDeviceCounters {
    pub reads_completed: AtomicU64,
    pub sectors_read: AtomicU64,
    pub writes_completed: AtomicU64,
    pub sectors_written: AtomicU64,
}

impl BlockDeviceCounters {
    pub const fn new() -> Self {
        Self {
            reads_completed: AtomicU64::new(0),
            sectors_read: AtomicU64::new(0),
            writes_completed: AtomicU64::new(0),
            sectors_written: AtomicU64::new(0),
        }
    }

    /// Records completed read operations and sector count.
    pub fn record_read(&self, sectors: u64) {
        self.reads_completed.fetch_add(1, Ordering::Relaxed);
        self.sectors_read.fetch_add(sectors, Ordering::Relaxed);
    }

    /// Records completed write operations and sector count.
    pub fn record_write(&self, sectors: u64) {
        self.writes_completed.fetch_add(1, Ordering::Relaxed);
        self.sectors_written.fetch_add(sectors, Ordering::Relaxed);
    }

    /// Generates a [`BlockDeviceStats`] snapshot for `/proc/diskstats`.
    pub fn to_stats(&self) -> BlockDeviceStats {
        BlockDeviceStats {
            reads_completed: self.reads_completed.load(Ordering::Relaxed),
            sectors_read: self.sectors_read.load(Ordering::Relaxed),
            writes_completed: self.writes_completed.load(Ordering::Relaxed),
            sectors_written: self.sectors_written.load(Ordering::Relaxed),
            ..Default::default()
        }
    }
}
