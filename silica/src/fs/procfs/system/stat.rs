// SPDX-License-Identifier: GPL-2.0

//! `/proc/stat` generator.

use alloc::{string::String, vec::Vec};
use core::fmt::Write;

pub fn generate_stat() -> Vec<u8> {
    let uptime_ticks = crate::sched::now();
    let procs = crate::proc::allproc_count();
    let num_cpus = ostd::cpu::num_cpus();
    let mut nr_running = 0;
    for i in 0..num_cpus {
        nr_running += crate::sched::nr_running_on(ostd::cpu::CpuId::new(i as u32));
    }
    if nr_running == 0 {
        nr_running = 1;
    }

    let mut s = String::new();
    let _ = writeln!(s, "cpu  {} 0 10 1000 0 0 0 0 0 0", uptime_ticks);
    for cpu in 0..num_cpus {
        let _ = writeln!(s, "cpu{} {} 0 10 1000 0 0 0 0 0 0", cpu, uptime_ticks / (num_cpus as u64).max(1));
    }
    let _ = writeln!(s, "btime 1700000000");
    let _ = writeln!(s, "processes {}", procs);
    let _ = writeln!(s, "procs_running {}", nr_running);
    let _ = writeln!(s, "procs_blocked 0");
    s.into_bytes()
}
