// SPDX-License-Identifier: GPL-2.0

//! `/proc/loadavg`.
//!
//! Load average plus runnable-task and PID-allocation statistics, following
//! the Linux format: `<loadavg> <runnable>/<total_threads> <last_pid>`.

use alloc::{format, sync::Arc, vec::Vec};

use crate::fs::procfs::template::{ProcFileOps, file_node};
use crate::fs::vfs::INode;

/// Content generator of the `/proc/loadavg` synthetic file.
pub struct LoadAvgFileOps;

impl ProcFileOps for LoadAvgFileOps {
    fn generate_content(&self) -> Vec<u8> {
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
            nr_threads += threads.max(1);
            if p.pid.as_u32() > last_pid {
                last_pid = p.pid.as_u32();
            }
        });
        if nr_threads == 0 {
            nr_threads = 1;
        }

        format!("{load} {nr_running}/{nr_threads} {last_pid}\n").into_bytes()
    }
}

impl LoadAvgFileOps {
    /// Creates the `/proc/loadavg` VFS inode.
    pub(crate) fn new_inode() -> Arc<INode> {
        file_node(Self, super::PROC_ROOT_INO_LOADAVG)
    }
}
