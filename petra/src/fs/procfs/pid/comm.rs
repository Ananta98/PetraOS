// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/comm` — thread name as a newline-terminated string.

use alloc::{format, vec::Vec};

use crate::{
    fs::procfs::template::ProcFileOps,
    proc::{Pid, allproc_find},
};

/// Content operations of the per-process comm file.
pub struct PidCommFileOps {
    pub pid: Pid,
}

impl ProcFileOps for PidCommFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let Some(proc) = allproc_find(self.pid) else {
            return Vec::new();
        };
        format!("{}\n", proc.comm_name()).into_bytes()
    }
}
