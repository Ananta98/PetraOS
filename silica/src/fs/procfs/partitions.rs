// SPDX-License-Identifier: GPL-2.0

//! `/proc/partitions`.
//!
//! Block devices known to the kernel, in the Linux header + rows format.

use alloc::{string::String, sync::Arc, vec::Vec};
use core::fmt::Write;

use crate::fs::procfs::template::{ProcFileOps, file_node};
use crate::fs::vfs::INode;

/// Content generator of the `/proc/partitions` synthetic file.
pub struct PartitionsFileOps;

impl ProcFileOps for PartitionsFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let mut s = String::new();
        let _ = writeln!(s, "major minor  #blocks  name\n");
        let blkdevs = crate::drivers::get_blkdevs();

        for dev in blkdevs {
            let (major, minor) = dev.device_number();
            let name = dev.name();
            let blocks = dev.capacity() / 1024;

            let _ = writeln!(s, "{:4} {:7} {:10} {}", major, minor, blocks, name);
        }

        s.into_bytes()
    }
}

impl PartitionsFileOps {
    /// Creates the `/proc/partitions` VFS inode.
    pub(crate) fn new_inode() -> Arc<INode> {
        file_node(Self, super::PROC_ROOT_INO_PARTITIONS)
    }
}
