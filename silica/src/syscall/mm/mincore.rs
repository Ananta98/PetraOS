// SPDX-License-Identifier: GPL-2.0

//! `mincore(2)` system call (Syscall #27 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
};

/// System call `mincore(addr, len, vec)` (Syscall #27 on x86_64).
pub fn sys_mincore(
    addr: usize,
    len: usize,
    vec_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if len == 0 {
        return Ok(0);
    }
    let Some(page_len) = len.checked_next_multiple_of(ostd::mm::PAGE_SIZE) else {
        crate::return_errno!(ENOMEM, "the length overflows");
    };
    let num_pages = page_len / ostd::mm::PAGE_SIZE;
    let proc = Thread::proc_or_esrch()?;
    let mut vec = alloc::vec![0u8; num_pages];
    let range = addr..addr.checked_add(page_len).ok_or(Errno::ENOMEM)?;
    proc.vmspace().mincore(range, &mut vec)?;
    UserPtr::new(vec_ptr).write_bytes(&proc.vmspace(), &vec)?;
    Ok(0)
}
