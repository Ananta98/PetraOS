// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/stat` generator.

use alloc::{format, sync::Arc, vec::Vec};

use crate::proc::{Proc, ProcState};

pub fn generate_proc_stat(proc: &Arc<Proc>) -> Vec<u8> {
    let inner = proc.inner.lock();
    let state_char = match inner.state {
        ProcState::Alive => 'S',
        ProcState::Zombie => 'Z',
    };
    let ppid = inner.ppid.map(|p| p.as_u32()).unwrap_or(0);
    let pgrp = inner.pgid.unwrap_or(proc.pid).as_u32();
    let sid = inner.sid.unwrap_or(proc.pid).as_u32();
    let num_threads = inner.threads.len();
    drop(inner);

    format!(
        "{} ({}) {} {} {} {} 0 0 0 0 0 0 0 0 0 20 0 {} 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n",
        proc.pid.as_u32(),
        proc.comm_name(),
        state_char,
        ppid,
        pgrp,
        sid,
        num_threads
    )
    .into_bytes()
}
