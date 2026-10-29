// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/maps` — process memory mappings.

use alloc::{format, vec::Vec};
use ostd::mm::MAX_USERSPACE_VADDR;

use crate::{
    fs::procfs::template::ProcFileOps,
    proc::{Pid, allproc_find, lifecycle::USER_STACK_TOP},
    vm::perms::VmPerms,
};

/// Content operations for `/proc/<pid>/maps`.
pub struct PidMapsFileOps {
    pub pid: Pid,
}

impl ProcFileOps for PidMapsFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let Some(proc) = allproc_find(self.pid) else {
            return Vec::new();
        };

        let vmar = proc.vmspace();
        let query = vmar.mappings_in(0..MAX_USERSPACE_VADDR);
        let start_brk = vmar.start_brk();
        let mut buf = Vec::new();

        for mapping in query.iter() {
            let start = mapping.start();
            let end = mapping.end();
            let perms = mapping.perms();
            let r = if perms.contains(VmPerms::READ) { 'r' } else { '-' };
            let w = if perms.contains(VmPerms::WRITE) { 'w' } else { '-' };
            let x = if perms.contains(VmPerms::EXEC) { 'x' } else { '-' };
            let s = if mapping.is_shared() { 's' } else { 'p' };
            let offset = mapping.backing_offset();

            let is_stack = end == USER_STACK_TOP;
            let is_heap = start_brk != 0 && start == start_brk;
            let label = if is_stack {
                "       [stack]"
            } else if is_heap {
                "       [heap]"
            } else {
                ""
            };

            let line = format!(
                "{:08x}-{:08x} {}{}{}{} {:08x} 00:00 0{}\n",
                start, end, r, w, x, s, offset, label
            );
            buf.extend_from_slice(line.as_bytes());
        }

        buf
    }
}
