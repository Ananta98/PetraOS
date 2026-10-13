// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/cwd` symlink operations.

use alloc::string::String;

use crate::{
    api::errno::Result,
    fs::procfs::template::ProcSymOps,
};

pub struct ProcPidCwdSymlinkOps;

impl ProcSymOps for ProcPidCwdSymlinkOps {
    fn readlink(&self) -> Result<String> {
        Ok(String::from("/"))
    }
}
