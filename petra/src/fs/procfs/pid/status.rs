// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/status` — human-readable process status.
//!
//! Follows the Linux field layout (`Name`, `State`, `Tgid`, `Pid`, `PPid`,
//! `Uid`, `Gid`, `FDSize`, `Groups`, `NSpgid`, `NSsid`, `Threads`) so
//! tooling like `ps` can consume it directly.

use alloc::{string::String, vec::Vec};
use core::fmt::Write;

use crate::{
    fs::procfs::template::ProcFileOps,
    proc::{Pid, ProcState, allproc_find},
};

/// Content operations of the per-process status file.
pub struct PidStatusFileOps {
    pub pid: Pid,
}

impl ProcFileOps for PidStatusFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let Some(proc) = allproc_find(self.pid) else {
            return Vec::new();
        };

        let mut s = String::new();
        let inner = proc.inner.lock();
        let state_char = match inner.state {
            ProcState::Alive => 'S',
            ProcState::Zombie => 'Z',
        };
        let ppid = inner.ppid.map(|p| p.as_u32()).unwrap_or(0);
        let pgrp = inner.pgid.unwrap_or(proc.pid).as_u32();
        let sid = inner.sid.unwrap_or(proc.pid).as_u32();
        let threads_count = inner.threads.len();
        drop(inner);

        let cred = proc.cred();
        let ruid = cred.cr_ruid.as_u32();
        let euid = cred.cr_uid.as_u32();
        let rgid = cred.cr_rgid.as_u32();
        let egid = cred.cr_gid.as_u32();

        let _ = writeln!(s, "Name:\t{}", proc.comm_name());
        let _ = writeln!(s, "State:\t{} (sleeping)", state_char);
        let _ = writeln!(s, "Tgid:\t{}", proc.pid.as_u32());
        let _ = writeln!(s, "Pid:\t{}", proc.pid.as_u32());
        let _ = writeln!(s, "PPid:\t{}", ppid);
        let _ = writeln!(s, "Uid:\t{}\t{}\t{}\t{}", ruid, euid, euid, euid);
        let _ = writeln!(s, "Gid:\t{}\t{}\t{}\t{}", rgid, egid, egid, egid);
        let _ = writeln!(s, "FDSize:\t64");
        let _ = writeln!(s, "Groups:\t{}", egid);
        let _ = writeln!(s, "NSpgid:\t{}", pgrp);
        let _ = writeln!(s, "NSsid:\t{}", sid);
        let _ = writeln!(s, "Threads:\t{}", threads_count);
        s.into_bytes()
    }
}
