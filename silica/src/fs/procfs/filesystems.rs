// SPDX-License-Identifier: GPL-2.0

//! `/proc/filesystems`.
//!
//! Enumerates the filesystem drivers the kernel knows about, in the
//! Linux-compatible format user land tools (`mount(8)`, `df(1)`) parse:
//! nodev entries carry a leading "nodev" separator, block-backed ones a
//! leading tab.

use alloc::vec::Vec;

use alloc::sync::Arc;

use crate::fs::procfs::template::{ProcFileOps, file_node};
use crate::fs::vfs::INode;

/// Content generator of the `/proc/filesystems` synthetic file.
pub struct FileSystemsFileOps;

impl ProcFileOps for FileSystemsFileOps {
    fn generate_content(&self) -> Vec<u8> {
        b"\
nodev\tdevfs
nodev\tproc
nodev\tsysfs
nodev\ttmpfs
nodev\tdevpts
nodev\tcgroup
nodev\tcgroup2
nodev\tpipefs
\text4
\texfat
"
        .to_vec()
    }
}

impl FileSystemsFileOps {
    /// Creates the `/proc/filesystems` VFS inode.
    pub(crate) fn new_inode() -> Arc<INode> {
        file_node(Self, super::PROC_ROOT_INO_FILESYSTEMS)
    }
}
