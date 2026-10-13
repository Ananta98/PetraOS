// SPDX-License-Identifier: GPL-2.0

//! `/proc/self` dynamic symlink resolving to the caller's PID.

use alloc::{format, string::String};

use crate::{
    api::errno::Result,
    proc::thread::Thread,
};

use super::template::ProcSymOps;

/// `/proc/self` dynamic symlink operations.
pub struct ProcSelfSymlinkOps;

impl ProcSymOps for ProcSelfSymlinkOps {
    fn readlink(&self) -> Result<String> {
        let pid = Thread::current_proc()
            .map(|p| p.pid.as_u32())
            .unwrap_or(1);
        Ok(format!("{}", pid))
    }
}
