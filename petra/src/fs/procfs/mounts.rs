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
        let mut content = String::new();
        if mounts.is_empty() {
            let _ = writeln!(content, "rootfs / rootfs rw 0 0");
            let _ = writeln!(content, "devfs /dev devfs rw,nosuid,relatime 0 0");
            let _ = writeln!(content, "proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0");
            let _ = writeln!(content, "sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0");
            return content.into_bytes();
        }

        let is_chrooted = crate::proc::current_proc()
            .and_then(|p| p.root.lock().clone())
            .is_some();
        let has_sysroot = mounts.iter().any(|m| m.target == "/sysroot");

        for m in &mounts {
            if is_chrooted && has_sysroot {
                write_sysroot_mount(&mut content, m);
            } else {
                let _ = writeln!(
                    content,
                    "{} {} {} {} 0 0",
                    m.source, m.target, m.fstype, m.options
                );
            }
        }
        content.into_bytes()
    }
}

fn write_sysroot_mount(content: &mut String, mount: &crate::fs::vfs::mount::MountRecord) {
    if mount.target == "/sysroot" {
        let _ = writeln!(
            content,
            "{} / {} {} 0 0",
            mount.source, mount.fstype, mount.options
        );
    } else if let Some(subpath) = mount.target.strip_prefix("/sysroot/") {
        let _ = writeln!(
            content,
            "{} /{} {} {} 0 0",
            mount.source, subpath, mount.fstype, mount.options
        );
    }
}

impl MountsFileOps {
    /// Creates the `/proc/mounts` VFS inode.
    pub(crate) fn new_inode() -> Arc<INode> {
        file_node(Self, super::PROC_ROOT_INO_MOUNTS)
    }
}
