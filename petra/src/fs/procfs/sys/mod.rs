// SPDX-License-Identifier: GPL-2.0

//! `/proc/sys` — kernel parameter hierarchy.
//!
//! The sysctl surface exposed to user space. Currently only `kernel/` is
//! populated; the directory layer is table-driven so further subtrees can be
//! added declaratively. Values follow Linux `/proc/sys` semantics (readable
//! by everyone, tunables writable by the owner).

pub mod kernel;

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    fs::procfs::template::{ProcDirOps, dir_node},
    fs::vfs::INode,
    uapi::errno::Result,
};

pub use kernel::{get_domainname, get_hostname, set_domainname, set_hostname};

/// `/proc/sys` directory node operations: a pure table over subtrees.
struct ProcSysDirOps;

/// Inode number allocation for `/proc/sys`.
const PROC_SYS_INO: usize = 2000;
const PROC_SYS_KERNEL_INO: usize = 2001;

impl ProcDirOps for ProcSysDirOps {
    fn lookup_child(&self, name: &str) -> Result<Arc<INode>> {
        match name {
            "kernel" => Ok(dir_node(kernel::ProcSysKernelDirOps, PROC_SYS_KERNEL_INO)),
            _ => crate::return_errno!(ENOENT, "unknown entry in /proc/sys"),
        }
    }

    fn list_entries(&self) -> Vec<String> {
        alloc::vec![String::from("kernel")]
    }
}

/// Creates the `/proc/sys` VFS inode.
pub(crate) fn new_sys_inode() -> Arc<INode> {
    dir_node(ProcSysDirOps, PROC_SYS_INO)
}
