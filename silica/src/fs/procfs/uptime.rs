// SPDX-License-Identifier: GPL-2.0

//! `/proc/uptime`.
//!
//! Seconds since boot, reported twice (`uptime idle_time`) as user land
//! tooling expects.

use alloc::{format, sync::Arc, vec::Vec};

use crate::fs::procfs::template::{ProcFileOps, file_node};
use crate::fs::vfs::INode;

/// Content generator of the `/proc/uptime` synthetic file.
pub struct UptimeFileOps;

impl ProcFileOps for UptimeFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let uptime = crate::time::uptime_secs();
        format!("{}.00 {}.00\n", uptime, uptime).into_bytes()
    }
}

impl UptimeFileOps {
    /// Creates the `/proc/uptime` VFS inode.
    pub(crate) fn new_inode() -> Arc<INode> {
        file_node(Self, super::PROC_ROOT_INO_UPTIME)
    }
}
