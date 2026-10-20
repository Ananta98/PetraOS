// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/cmdline` — argv as a NUL-separated list.

use alloc::vec::Vec;

use crate::{
    fs::procfs::template::ProcFileOps,
    proc::{Pid, allproc_find},
};

/// Content operations of the per-process command-line file.
pub struct PidCmdLineFileOps {
    pub pid: Pid,
}

impl ProcFileOps for PidCmdLineFileOps {
    fn generate_content(&self) -> Vec<u8> {
        // The process may exit between directory lookup and open; report an
        // empty file rather than failing the read.
        let Some(proc) = allproc_find(self.pid) else {
            return Vec::new();
        };
        let mut buf = proc.comm_name().into_bytes();
        buf.push(0);
        buf
    }
}
