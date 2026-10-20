// SPDX-License-Identifier: GPL-2.0

//! `/proc/cpuinfo`.
//!
//! Architecture-specific CPU descriptions; the text itself is produced by
//! the `arch` subsystem so each architecture reports its own feature set.

use alloc::vec::Vec;

use alloc::sync::Arc;

use crate::fs::procfs::template::{ProcFileOps, file_node};
use crate::fs::vfs::INode;

/// Content generator of the `/proc/cpuinfo` synthetic file.
pub struct CpuInfoFileOps;

impl ProcFileOps for CpuInfoFileOps {
    fn generate_content(&self) -> Vec<u8> {
        crate::arch::cpuinfo::generate_cpuinfo()
    }
}

impl CpuInfoFileOps {
    /// Creates the `/proc/cpuinfo` VFS inode.
    pub(crate) fn new_inode() -> Arc<INode> {
        file_node(Self, super::PROC_ROOT_INO_CPUINFO)
    }
}
