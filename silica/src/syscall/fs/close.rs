// SPDX-License-Identifier: GPL-2.0

//! The `close` system call (Syscall #3 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{proc::thread::Thread, uapi::errno::Result};

/// System call `close(int fd)` (Syscall #3).
pub fn sys_close(fd: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    proc.fd_table.lock().close(fd)?;
    Ok(0)
}

/// System call `close_range(unsigned int first, unsigned int last, unsigned int flags)` (Syscall #436).
pub fn sys_close_range(first: u32, last: u32, flags: u32, _ctx: &mut UserContext) -> Result<usize> {
    if (flags & !(crate::uapi::fd::CLOSE_RANGE_UNSHARE | crate::uapi::fd::CLOSE_RANGE_CLOEXEC)) != 0 {
        return Err(crate::uapi::errno::Errno::EINVAL);
    }
    let cloexec = (flags & crate::uapi::fd::CLOSE_RANGE_CLOEXEC) != 0;
    let proc = Thread::proc_or_esrch()?;
    proc.fd_table.lock().close_range(first, last, cloexec)?;
    Ok(0)
}

