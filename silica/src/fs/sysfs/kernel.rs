// SPDX-License-Identifier: GPL-2.0

//! `/sys/kernel` kernel subsystem nodes.

use crate::fs::sysfs::kobject::sys_tree;

/// Initializes `/sys/kernel` synthetic attributes.
pub fn init_kernel_subsystem() {
    let tree = sys_tree();
    let _ = tree.kernel_dir.add_static_attr("uevent_seqnum", b"1\n");
    let _ = tree.kernel_dir.add_static_attr("profiling", b"0\n");
}
