// SPDX-License-Identifier: GPL-2.0

//! Control Group v2 (cgroupv2) pseudo-filesystem.
//!
//! Provides the unified cgroup v2 filesystem (`cgroup2`) hierarchy, typically mounted
//! at `/sys/fs/cgroup`.
//!
//! Implements the standard cgroup v2 control files, dynamic sub-group directory creation
//! (`mkdir`/`rmdir`), process migration via `cgroup.procs`, and controller delegations
//! (`cgroup.controllers`, `cgroup.subtree_control`).

pub mod fs;
pub mod node;
pub mod tree;

pub use fs::Cgroup2Fs;
pub use tree::{CgroupNode, default_cgroup_tree};

/// Initializes and mounts/registers cgroup v2 if needed at boot.
pub fn init() {
    // Seed default root cgroup hierarchy
    let _ = default_cgroup_tree();
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::fs::vfs::{FileSystem, Mode, OpenFlags};
    use ostd::prelude::ktest;

    #[ktest]
    fn cgroup2_hierarchy_and_controls() {
        let fs = Cgroup2Fs::new();
        let root = fs.root().expect("cgroup2 root");

        // Lookup cgroup.controllers
        let ctrl_node = root
            .node_ops
            .lookup("cgroup.controllers")
            .expect("lookup cgroup.controllers");
        let file = ctrl_node.node_ops.open(OpenFlags::READ).expect("open cgroup.controllers");
        let mut buf = [0u8; 64];
        let n = file.read(0, &mut buf).expect("read cgroup.controllers");
        let content = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(content.contains("cpu") && content.contains("memory") && content.contains("pids"));

        // Lookup cgroup.procs
        let procs_node = root
            .node_ops
            .lookup("cgroup.procs")
            .expect("lookup cgroup.procs");
        let pfile = procs_node.node_ops.open(OpenFlags::READ | OpenFlags::WRITE).expect("open cgroup.procs");
        let mut pbuf = [0u8; 128];
        let pn = pfile.read(0, &mut pbuf).expect("read cgroup.procs");
        // During early ktest boot there might not be userland processes yet or there may be init
        let _ = pn;

        // Test mkdir sub-cgroup
        let sub_node = root
            .node_ops
            .mkdir("system.slice", Mode::DIR | Mode::from_bits_truncate(0o755))
            .expect("mkdir system.slice");
        assert!(sub_node.is_dir());

        // Verify sub-cgroup has its own cgroup.procs and cgroup.controllers
        let sub_procs = sub_node
            .node_ops
            .lookup("cgroup.procs")
            .expect("lookup sub cgroup.procs");
        assert!(sub_procs.is_file());

        // Write PID 1 to sub-cgroup cgroup.procs if PID 1 is present, or verify empty
        let sub_procs_file = sub_procs
            .node_ops
            .open(OpenFlags::READ | OpenFlags::WRITE)
            .expect("open sub cgroup.procs");

        let mut sp_buf = [0u8; 64];
        let spn = sub_procs_file.read(0, &mut sp_buf).expect("read empty sub cgroup.procs");
        assert_eq!(spn, 0);

        // Test rmdir on empty sub-cgroup succeeds
        root.node_ops.rmdir("system.slice").expect("rmdir system.slice");
        assert!(root.node_ops.lookup("system.slice").is_err());
    }
}
