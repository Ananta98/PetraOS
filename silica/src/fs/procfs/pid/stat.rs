// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/stat` generator.

use alloc::{format, sync::Arc, vec::Vec};

use crate::proc::{Proc, ProcState};

pub fn generate_proc_stat(proc: &Arc<Proc>) -> Vec<u8> {
    generate_proc_stat_for_task(proc, None)
}

pub fn generate_proc_stat_for_task(proc: &Arc<Proc>, tid: Option<u32>) -> Vec<u8> {
    let inner = proc.inner.lock();
    let state_char = match inner.state {
        ProcState::Alive => 'S',
        ProcState::Zombie => 'Z',
    };
    let ppid = inner.ppid.map(|p| p.as_u32()).unwrap_or(0);
    let pgrp = inner.pgid.unwrap_or(proc.pid).as_u32();
    let sid = inner.sid.unwrap_or(proc.pid).as_u32();
    let num_threads = inner.threads.len().max(1);
    drop(inner);

    let pid_val = tid.unwrap_or_else(|| proc.pid.as_u32());
    let vsize = proc.vmspace().total_mapped_size();
    let rss = vsize / 4096;

    // Linux /proc/[pid]/stat has 52 standard fields:
    // 1:pid 2:(comm) 3:state 4:ppid 5:pgrp 6:sid 7:tty_nr 8:tpgid 9:flags
    // 10:minflt 11:cminflt 12:majflt 13:cmajflt 14:utime 15:stime 16:cutime 17:cstime
    // 18:priority(20) 19:nice(0) 20:num_threads 21:itrealvalue(0) 22:starttime(0)
    // 23:vsize 24:rss 25..38:0 39:processor(0) 40..52:0
    format!(
        "{} ({}) {} {} {} {} 0 0 0 0 0 0 0 0 0 0 0 20 0 {} 0 0 {} {} 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n",
        pid_val,
        proc.comm_name(),
        state_char,
        ppid,
        pgrp,
        sid,
        num_threads,
        vsize,
        rss,
    )
    .into_bytes()
}
