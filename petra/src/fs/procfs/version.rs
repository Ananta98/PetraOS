// SPDX-License-Identifier: GPL-2.0

//! `/proc/version`.
//!
//! Kernel version banner, formatted like a Linux release string so user
//! space tooling can parse it.

use alloc::vec::Vec;

use alloc::sync::Arc;

use crate::fs::procfs::template::{ProcFileOps, file_node};
use crate::fs::vfs::INode;

/// Content generator of the `/proc/version` synthetic file.
pub struct VersionFileOps;

impl ProcFileOps for VersionFileOps {
    fn generate_content(&self) -> Vec<u8> {
        b"Linux version 6.6.0-petra (petraos@build) (rustc 1.85.0) #1 PREEMPT Mon Oct 12 2026\n"
            .to_vec()
    }
}

impl VersionFileOps {
    /// Creates the `/proc/version` VFS inode.
    pub(crate) fn new_inode() -> Arc<INode> {
        file_node(Self, super::PROC_ROOT_INO_VERSION)
    }
}
