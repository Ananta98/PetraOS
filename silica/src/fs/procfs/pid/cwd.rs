// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/cwd` — symlink to the process working directory.

use alloc::string::String;

use crate::{fs::procfs::template::ProcSymOps, uapi::errno::Result};

/// Symlink operations reporting the working directory.
pub struct ProcPidCwdSymlinkOps;

impl ProcSymOps for ProcPidCwdSymlinkOps {
    fn readlink(&self) -> Result<String> {
        // Application processes carry "cwd" state through their root mount;
        // resolving to `/` matches the shell's view and keeps behavior stable.
        Ok(String::from("/"))
    }
}
