// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/exe` — symlink to the executed binary.

use alloc::{format, string::String};

use crate::{
    api::errno::Errno,
    api::errno::Result,
    fs::procfs::template::ProcSymOps,
    proc::{Pid, allproc_find},
};

/// Symlink operations resolving to the executable path.
pub struct ProcPidExeSymlinkOps {
    pub pid: Pid,
}

impl ProcSymOps for ProcPidExeSymlinkOps {
    fn readlink(&self) -> Result<String> {
        let proc = allproc_find(self.pid).ok_or(Errno::ENOENT)?;
        let exe = proc.exe_path();
        if exe.starts_with('/') {
            Ok(exe)
        } else {
            Ok(format!("/{}", exe))
        }
    }
}
