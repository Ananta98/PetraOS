// SPDX-License-Identifier: GPL-2.0

//! `/proc/stat`.
//!
//! Global and per-CPU statistics in the Linux format: aggregate cpu line,
//! one `cpu<N>` line per CPU, boot time, process accounting.

use alloc::{string::String, sync::Arc, vec::Vec};
use core::fmt::Write;

use crate::fs::procfs::template::{ProcFileOps, file_node};
use crate::fs::vfs::INode;

/// Content generator of the `/proc/stat` synthetic file.
pub struct StatFileOps;

impl ProcFileOps for StatFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let procs = crate::proc::allproc_count();
        let num_cpus = ostd::cpu::num_cpus();
        let mut nr_running = 0;

        let mut total_user: u64 = 0;
        let mut total_nice: u64 = 0;
        let mut total_system: u64 = 0;
        let mut total_idle: u64 = 0;

        let mut per_cpu_stats = Vec::new();

        for i in 0..num_cpus {
            let cpu_id = ostd::cpu::CpuId::new(i as u32);
            nr_running += crate::sched::nr_running_on(cpu_id);

            let (user, nice, system, idle) = if let Some(snap) = crate::sched::cpu_stats_on(cpu_id)
            {
                // Linux proc stat cpu ticks are in USER_HZ (100 Hz = 10_000_000 ns per tick).
                let user_ticks = snap.exec_ns / 10_000_000;
                let mut idle_ticks = snap.idle_ns / 10_000_000;
                if user_ticks == 0 && idle_ticks == 0 {
                    let cpu_uptime = crate::sched::now() / (num_cpus as u64).max(1);
                    idle_ticks = cpu_uptime.max(1);
                }
                (user_ticks, 0, 0, idle_ticks)
            } else {
                let cpu_uptime = crate::sched::now() / (num_cpus as u64).max(1);
                (0, 0, 0, cpu_uptime.max(1))
            };

            total_user += user;
            total_nice += nice;
            total_system += system;
            total_idle += idle;

            per_cpu_stats.push((i, user, nice, system, idle));
        }

        if nr_running == 0 {
            nr_running = 1;
        }

        let mut s = String::new();
        let _ = writeln!(
            s,
            "cpu  {} {} {} {} 0 0 0 0 0 0",
            total_user, total_nice, total_system, total_idle
        );
        for (cpu, user, nice, system, idle) in per_cpu_stats {
            let _ = writeln!(
                s,
                "cpu{} {} {} {} {} 0 0 0 0 0 0",
                cpu, user, nice, system, idle
            );
        }
        let _ = writeln!(s, "btime 1700000000");
        let _ = writeln!(s, "processes {}", procs);
        let _ = writeln!(s, "procs_running {}", nr_running);
        let _ = writeln!(s, "procs_blocked 0");
        s.into_bytes()
    }
}

impl StatFileOps {
    /// Creates the `/proc/stat` VFS inode.
    pub(crate) fn new_inode() -> Arc<INode> {
        file_node(Self, super::PROC_ROOT_INO_STAT)
    }
}
