// SPDX-License-Identifier: GPL-2.0

//! `mprotect(2)` system call.

use ostd::{arch::cpu::context::UserContext, mm::PAGE_SIZE};

use crate::{
    proc::thread::Thread,
    uapi::errno::{Errno, Result},
    vm::perms::VmPerms,
};

/// System call `mprotect(addr, len, prot)` (Syscall #10 on x86_64).
pub fn sys_mprotect(
    addr: usize,
    len: usize,
    prot_val: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if !addr.is_multiple_of(PAGE_SIZE) {
        crate::return_errno!(EINVAL, "mprotect: addr {addr:#x} is not page-aligned");
    }
    if len == 0 {
        return Ok(0);
    }
    let aligned_len = len
        .checked_next_multiple_of(PAGE_SIZE)
        .ok_or(Errno::EINVAL)?;
    let end = addr.checked_add(aligned_len).ok_or(Errno::EINVAL)?;

    let perms = VmPerms::from_prot(prot_val)?;

    let proc = Thread::proc_or_esrch()?;
    proc.vmspace().mprotect(perms, addr..end)?;

    Ok(0)
}
