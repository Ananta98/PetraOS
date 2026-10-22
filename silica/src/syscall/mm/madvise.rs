// SPDX-License-Identifier: GPL-2.0

//! `madvise(2)` system call.

use ostd::{arch::cpu::context::UserContext, mm::PAGE_SIZE};

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    vm::flags::MadviseAdvice,
};

/// System call `madvise(addr, len, advice)` (Syscall #28 on x86_64).
pub fn sys_madvise(
    addr: usize,
    len: usize,
    advice_val: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let advice = MadviseAdvice::from_raw(advice_val)?;

    if !addr.is_multiple_of(PAGE_SIZE) {
        crate::return_errno!(EINVAL, "madvise: addr {addr:#x} is not page-aligned");
    }
    if len == 0 {
        return Ok(0);
    }
    let aligned_len = len
        .checked_next_multiple_of(PAGE_SIZE)
        .ok_or(Errno::EINVAL)?;
    let end = addr.checked_add(aligned_len).ok_or(Errno::EINVAL)?;

    let proc = Thread::proc_or_esrch()?;
    proc.vmspace().madvise(advice, addr..end)?;

    Ok(0)
}
