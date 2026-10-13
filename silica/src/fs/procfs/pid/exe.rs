// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/exe` symlink operations.

use alloc::{format, string::String};

use crate::{
    api::errno::{Errno, Result},
    fs::procfs::template::ProcSymOps,
    proc::tree::{allproc_find, Pid},
};

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
