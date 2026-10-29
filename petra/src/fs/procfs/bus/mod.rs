// SPDX-License-Identifier: GPL-2.0

//! `/proc/bus` directory hierarchy.

pub mod input;

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    fs::procfs::template::{ProcDirOps, dir_node},
    fs::vfs::INode,
    uapi::errno::Result,
};

pub(crate) const PROC_BUS_INO: usize = 3000;

/// Directory operations for `/proc/bus`.
pub struct ProcBusDirOps;

impl ProcDirOps for ProcBusDirOps {
    fn lookup_child(&self, name: &str) -> Result<Arc<INode>> {
        match name {
            "input" => Ok(input::new_input_inode()),
            _ => crate::return_errno!(ENOENT, "unknown entry in /proc/bus"),
        }
    }

    fn list_entries(&self) -> Vec<String> {
        alloc::vec![String::from("input")]
    }
}

/// Creates the `/proc/bus` VFS inode.
pub fn new_bus_inode() -> Arc<INode> {
    dir_node(ProcBusDirOps, PROC_BUS_INO)
}
