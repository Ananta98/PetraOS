// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/status` generator.

use alloc::{string::String, sync::Arc, vec::Vec};
use core::fmt::Write;

use crate::proc::{Proc, ProcState};

pub fn generate_proc_status(proc: &Arc<Proc>) -> Vec<u8> {
    let mut s = String::new();
    let inner = proc.inner.lock();
    let state_char = match inner.state {
        ProcState::Alive => 'S',
        ProcState::Zombie => 'Z',
    };
    let ppid = inner.ppid.map(|p| p.as_u32()).unwrap_or(0);
    let threads_count = inner.threads.len();
    drop(inner);

    let _ = writeln!(s, "Name:\t{}", proc.comm_name());
    let _ = writeln!(s, "State:\t{} (sleeping)", state_char);
    let _ = writeln!(s, "Tgid:\t{}", proc.pid.as_u32());
    let _ = writeln!(s, "Pid:\t{}", proc.pid.as_u32());
    let _ = writeln!(s, "PPid:\t{}", ppid);
    let _ = writeln!(s, "Uid:\t0\t0\t0\t0");
    let _ = writeln!(s, "Gid:\t0\t0\t0\t0");
    let _ = writeln!(s, "FDSize:\t64");
    let _ = writeln!(s, "Threads:\t{}", threads_count);
    s.into_bytes()
}
