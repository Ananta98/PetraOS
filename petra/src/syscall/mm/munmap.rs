// SPDX-License-Identifier: GPL-2.0

use crate::{
    proc::thread::Thread,
    uapi::errno::{Errno, Result},
};
use ostd::{
    arch::cpu::context::UserContext,
    mm::{MAX_USERSPACE_VADDR, PAGE_SIZE},
};

pub fn sys_munmap(addr: usize, len: usize, _ctx: &mut UserContext) -> Result<usize> {
    if len == 0 || !addr.is_multiple_of(PAGE_SIZE) {
        return Err(Errno::EINVAL);
    }
    let aligned_len = len
        .checked_next_multiple_of(PAGE_SIZE)
        .ok_or(Errno::EINVAL)?;
    let end = addr.checked_add(aligned_len).ok_or(Errno::EINVAL)?;
    if end > MAX_USERSPACE_VADDR {
        return Err(Errno::EINVAL);
    }
    let proc = Thread::proc_or_esrch()?;
    proc.vmspace().munmap(addr..end)?;
    Ok(0)
}
