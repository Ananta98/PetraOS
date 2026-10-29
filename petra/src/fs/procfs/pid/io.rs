// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/io` — per-process I/O accounting.
//!
//! Uses Linux task I/O accounting field names (`rchar`, `wchar`, `syscr`,
//! `syscw`, `read_bytes`, `write_bytes`, `cancelled_write_bytes`) so user
//! land tools like `iotop` parse the data.

use alloc::{string::String, vec::Vec};
use core::fmt::Write;
use core::sync::atomic::Ordering;

use crate::{
    fs::procfs::template::ProcFileOps,
    proc::{Pid, allproc_find},
};

/// Content operations of the per-process io accounting file.
pub struct PidIoFileOps {
    pub pid: Pid,
}

impl ProcFileOps for PidIoFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let Some(proc) = allproc_find(self.pid) else {
            return Vec::new();
        };

        let mut s = String::new();
        let stats = &proc.io_stats;

        let rchar = stats.rchar.load(Ordering::Relaxed);
        let wchar = stats.wchar.load(Ordering::Relaxed);
        let syscr = stats.syscr.load(Ordering::Relaxed);
        let syscw = stats.syscw.load(Ordering::Relaxed);
        let read_bytes = stats.read_bytes.load(Ordering::Relaxed);
        let write_bytes = stats.write_bytes.load(Ordering::Relaxed);
        let cancelled_write_bytes = stats.cancelled_write_bytes.load(Ordering::Relaxed);

        let _ = writeln!(s, "rchar: {}", rchar);
        let _ = writeln!(s, "wchar: {}", wchar);
        let _ = writeln!(s, "syscr: {}", syscr);
        let _ = writeln!(s, "syscw: {}", syscw);
        let _ = writeln!(s, "read_bytes: {}", read_bytes);
        let _ = writeln!(s, "write_bytes: {}", write_bytes);
        let _ = writeln!(s, "cancelled_write_bytes: {}", cancelled_write_bytes);

        s.into_bytes()
    }
}
