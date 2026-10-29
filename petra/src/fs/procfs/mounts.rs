// SPDX-License-Identifier: GPL-2.0

//! `/proc/mounts`.
//!
//! Flat listing of the mount table, in the classic `fstab`-like form
//! `source target fstype options 0 0` that user land tooling parses.

use alloc::{string::String, sync::Arc, vec::Vec};
use core::fmt::Write;

use crate::fs::procfs::template::{ProcFileOps, file_node};
use crate::fs::vfs::INode;

/// Content generator of the `/proc/mounts` synthetic file.
pub struct MountsFileOps;

impl ProcFileOps for MountsFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let mounts = crate::fs::vfs::mount::get_mounts();
        let mut s = String::new();
        if mounts.is_empty() {
            let _ = writeln!(s, "rootfs / rootfs rw 0 0");
            let _ = writeln!(s, "devfs /dev devfs rw,nosuid,relatime 0 0");
            let _ = writeln!(s, "proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0");
            let _ = writeln!(s, "sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0");
        } else {
            for m in &mounts {
                let _ = writeln!(
                    s,
                    "{} {} {} {} 0 0",
                    m.source, m.target, m.fstype, m.options
                );
            }
        }
        s.into_bytes()
    }
}

impl MountsFileOps {
    /// Creates the `/proc/mounts` VFS inode.
    pub(crate) fn new_inode() -> Arc<INode> {
        file_node(Self, super::PROC_ROOT_INO_MOUNTS)
    }
}
