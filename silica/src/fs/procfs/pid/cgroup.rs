// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/cgroup` — process cgroup membership.
//!
//! Exposes unified cgroup v2 format `0::<path>\n` consumed by systemd and proc tools.

use alloc::{format, vec::Vec};

use crate::{
    fs::{cgroup::default_cgroup_tree, procfs::template::ProcFileOps},
    proc::Pid,
};

/// Content operations for `/proc/<pid>/cgroup`.
pub struct PidCgroupFileOps {
    pub pid: Pid,
}

impl ProcFileOps for PidCgroupFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let tree = default_cgroup_tree();
        let path = tree.find_proc_cgroup(self.pid);
        // cgroup v2 unified hierarchy is always entry 0
        let s = format!("0::{}\n", path);
        s.into_bytes()
    }
}
