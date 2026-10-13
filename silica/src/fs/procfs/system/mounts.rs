// SPDX-License-Identifier: GPL-2.0

//! `/proc/mounts` generator.

use alloc::{string::String, vec::Vec};
use core::fmt::Write;

pub fn generate_mounts() -> Vec<u8> {
    let mounts = crate::fs::vfs::mount::get_mounts();
    if mounts.is_empty() {
        let mut s = String::new();
        s.push_str("rootfs / rootfs rw 0 0\n");
        s.push_str("devfs /dev devfs rw,nosuid,relatime 0 0\n");
        s.push_str("proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0\n");
        s.push_str("sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0\n");
        s.into_bytes()
    } else {
        let mut s = String::new();
        for m in &mounts {
            let _ = writeln!(s, "{} {} {} {} 0 0", m.source, m.target, m.fstype, m.options);
        }
        s.into_bytes()
    }
}
