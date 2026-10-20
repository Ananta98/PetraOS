// SPDX-License-Identifier: GPL-2.0

//! `/proc/self` dynamic symlink resolving to the caller's PID.
//!
//! Like its Linux counterpart, the target is materialed at `readlink` time
//! from the reading thread, so the "same" path resolves differently per
//! process.

use alloc::{format, string::String, sync::Arc};

use crate::{
    api::errno::Result,
    fs::{
        procfs::template::{ProcSymOps, sym_node},
        vfs::INode,
    },
    proc::thread::Thread,
};

/// Inode number of `/proc/self`; shared by all PID table slots because the
/// target itself is process-relative.
const PROC_SELF_INO: usize = 100;

/// `/proc/self` dynamic symlink operations.
pub struct ProcSelfSymlinkOps;

impl ProcSymOps for ProcSelfSymlinkOps {
    fn readlink(&self) -> Result<String> {
        let pid = Thread::current_proc().map(|p| p.pid.as_u32()).unwrap_or(1);
        Ok(format!("{}", pid))
    }
}

/// Creates the `/proc/self` VFS inode.
pub(crate) fn new_self_inode() -> Arc<INode> {
    sym_node(ProcSelfSymlinkOps, PROC_SELF_INO)
}
