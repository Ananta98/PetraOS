// SPDX-License-Identifier: GPL-2.0

//! `/proc/diskstats`.
//!
//! I/O statistics for every registered block device, following the standard
//! Linux `/proc/diskstats` field order.

use alloc::{string::String, sync::Arc, vec::Vec};
use core::fmt::Write;

use crate::fs::procfs::template::{ProcFileOps, file_node};
use crate::fs::vfs::INode;

/// Content generator of the `/proc/diskstats` synthetic file.
pub struct DiskStatsFileOps;

impl ProcFileOps for DiskStatsFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let mut s = String::new();
        let blkdevs = crate::drivers::get_blkdevs();

        for dev in blkdevs {
            let (major, minor) = dev.device_number();
            let name = dev.name();
            let stats = dev.stats();

            let _ = writeln!(
                s,
                "{:4} {:7} {} {} {} {} {} {} {} {} {} {} {} {}",
                major,
                minor,
                name,
                stats.reads_completed,
                stats.reads_merged,
                stats.sectors_read,
                stats.read_ms,
                stats.writes_completed,
                stats.writes_merged,
                stats.sectors_written,
                stats.write_ms,
                stats.io_in_progress,
                stats.io_ms,
                stats.weighted_io_ms,
            );
        }

        s.into_bytes()
    }
}

impl DiskStatsFileOps {
    /// Creates the `/proc/diskstats` VFS inode.
    pub(crate) fn new_inode() -> Arc<INode> {
        file_node(Self, super::PROC_ROOT_INO_DISKSTATS)
    }
}
