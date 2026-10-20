// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/stat` — process status, compact form.
//!
//! Reproduces the 52-field Linux `/proc/[pid]/stat` line. The `tid`
//! parameter selects which task the line describes: `None` renders the
//! leading task (`/proc/<pid>/stat`), `Some(tid)` renders a per-thread line
//! (`/proc/<pid>/task/<tid>/stat`).

use alloc::format;
use alloc::vec::Vec;

use crate::{
    fs::procfs::template::ProcFileOps,
    proc::{Pid, ProcState, allproc_find},
};

/// Content operations of the leading-task stat file.
pub struct PidStatFileOps {
    pub pid: Pid,
}

impl ProcFileOps for PidStatFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let Some(proc) = allproc_find(self.pid) else {
            return Vec::new();
        };
        format_stat(&proc, None)
    }
}

/// Content operations of a per-thread stat file under `task/<tid>/`.
pub struct TaskStatFileOps {
    pub pid: Pid,
    pub tid: u32,
}

impl ProcFileOps for TaskStatFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let Some(proc) = allproc_find(self.pid) else {
            return Vec::new();
        };
        format_stat(&proc, Some(self.tid))
    }
}

/// Renders one `stat` line for `tid` (or the leading task when `None`).
fn format_stat(proc: &alloc::sync::Arc<crate::proc::Proc>, tid: Option<u32>) -> Vec<u8> {
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
        "{} ({}) {} {} {} {} 0 0 0 0 0 0 0 0 0 0 0 20 0 {} 0 0 {} {} 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n",
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
