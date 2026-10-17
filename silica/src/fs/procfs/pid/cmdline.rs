// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/cmdline` file generator.

use alloc::vec::Vec;

use crate::proc::{allproc_find, Pid};

pub fn generate_proc_cmdline(pid: Pid) -> Vec<u8> {
    if let Some(p) = allproc_find(pid) {
        let mut buf = Vec::new();
        buf.extend_from_slice(p.comm_name().as_bytes());
        buf.push(0);
        buf
    } else {
        Vec::new()
    }
}
