// SPDX-License-Identifier: GPL-2.0

//! `/proc/stat` generator.

use alloc::{string::String, vec::Vec};
use core::fmt::Write;

pub fn generate_stat() -> Vec<u8> {
    let uptime_ticks = crate::sched::now();
    let procs = crate::proc::tree::allproc_count();
    let mut s = String::new();
    let _ = writeln!(s, "cpu  {} 0 10 1000 0 0 0 0 0 0", uptime_ticks);
    let _ = writeln!(s, "btime 1700000000");
    let _ = writeln!(s, "processes {}", procs);
    let _ = writeln!(s, "procs_running 1");
    let _ = writeln!(s, "procs_blocked 0");
    s.into_bytes()
}
