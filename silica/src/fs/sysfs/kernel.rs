// SPDX-License-Identifier: GPL-2.0

//! `/sys/kernel` — static kernel attributes.
//!
//! Registers the synthetic kernel-level parameters every Linux-style sysfs
//! carries. Device and class subsystems register elsewhere; this module only
//! seeds fixed content and is invoked once at boot via [`crate::fs::sysfs::
//! init`].

use super::kobject::sys_tree;

/// Seeds `/sys/kernel` synthetic attributes.
///
/// Idempotent: re-registration replaces the fixed content, so calling this
/// more than once is harmless.
pub(crate) fn init() {
    let tree = sys_tree();
    tree.kernel_dir().add_static_attr("uevent_seqnum", b"1\n");
    tree.kernel_dir().add_static_attr("profiling", b"0\n");
}
