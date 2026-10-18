// SPDX-License-Identifier: GPL-2.0

//! `/proc/loadavg` generator.

use alloc::{format, vec::Vec};

pub fn generate_loadavg() -> Vec<u8> {
    let load = crate::sched::loadavg();
    let num_cpus = ostd::cpu::num_cpus();
    let mut nr_running = 0;
    for i in 0..num_cpus {
        nr_running += crate::sched::nr_running_on(ostd::cpu::CpuId::new(i as u32));
    }
    if nr_running == 0 {
        nr_running = 1;
    }

    let mut nr_threads = 0;
    let mut last_pid = 1;
    crate::proc::allproc_for_each(|p| {
        let threads = p.inner.lock().threads.len();
        nr_threads += if threads == 0 { 1 } else { threads };
        if p.pid.as_u32() > last_pid {
            last_pid = p.pid.as_u32();
        }
    });
    if nr_threads == 0 {
        nr_threads = 1;
    }

    format!("{load} {nr_running}/{nr_threads} {last_pid}\n").into_bytes()
}
