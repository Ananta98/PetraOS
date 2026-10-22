// SPDX-License-Identifier: GPL-2.0

//! The `dup`, `dup2`, and `dup3` system calls (Syscalls #32, #33, and #292 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    fs::fd::FdFlags,
    proc::thread::Thread,
};

const O_CLOEXEC: i32 = 0o2000000;

/// System call `dup(int oldfd)` (Syscall #32).
pub fn sys_dup(oldfd: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let mut table = proc.fd_table.lock();
    let file = table.get(oldfd)?;
    let new_fd = table.alloc_fd(file, FdFlags::empty())?;
    Ok(new_fd as usize)
}

/// System call `dup2(int oldfd, int newfd)` (Syscall #33).
pub fn sys_dup2(oldfd: i32, newfd: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let mut table = proc.fd_table.lock();
    let res = table.dup2(oldfd, newfd, FdFlags::empty())?;
    Ok(res as usize)
}

/// System call `dup3(int oldfd, int newfd, int flags)` (Syscall #292).
pub fn sys_dup3(oldfd: i32, newfd: i32, flags: i32, _ctx: &mut UserContext) -> Result<usize> {
    if oldfd == newfd {
        return Err(Errno::EINVAL);
    }
    if (flags & !O_CLOEXEC) != 0 {
        return Err(Errno::EINVAL);
    }

    let mut fd_flags = FdFlags::empty();
    if (flags & O_CLOEXEC) != 0 {
        fd_flags |= FdFlags::FD_CLOEXEC;
    }

    let proc = Thread::proc_or_esrch()?;
    let mut table = proc.fd_table.lock();
    let res = table.dup2(oldfd, newfd, fd_flags)?;
    Ok(res as usize)
}
