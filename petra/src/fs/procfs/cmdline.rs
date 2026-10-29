// SPDX-License-Identifier: GPL-2.0

//! `/proc/cmdline`.
//!
//! Reports the kernel command-line arguments, NUL-separated as user space
//! expects (same convention as Linux).

use alloc::vec::Vec;

use alloc::sync::Arc;

use crate::fs::procfs::template::{ProcFileOps, file_node};
use crate::fs::vfs::INode;

/// Content generator of the `/proc/cmdline` synthetic file.
pub struct CmdLineFileOps;

impl ProcFileOps for CmdLineFileOps {
    fn generate_content(&self) -> Vec<u8> {
        b"console=tty0 root=/dev/ram0 init=/bin/sh\n".to_vec()
    }
}

impl CmdLineFileOps {
    /// Creates the `/proc/cmdline` VFS inode.
    pub(crate) fn new_inode() -> Arc<INode> {
        file_node(Self, super::PROC_ROOT_INO_CMDLINE)
    }
}
