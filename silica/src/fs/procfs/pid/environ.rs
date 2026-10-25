// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/environ` — process environment variables.

use alloc::vec::Vec;

use crate::{
    fs::procfs::template::ProcFileOps,
    proc::{Pid, allproc_find},
};

/// Content operations for `/proc/<pid>/environ`.
pub struct PidEnvironFileOps {
    pub pid: Pid,
}

impl ProcFileOps for PidEnvironFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let Some(proc) = allproc_find(self.pid) else {
            return Vec::new();
        };

        let inner = proc.inner.lock();
        let mut buf = Vec::new();
        for var in &inner.environ {
            buf.extend_from_slice(var.as_bytes());
            buf.push(0);
        }
        buf
    }
}
