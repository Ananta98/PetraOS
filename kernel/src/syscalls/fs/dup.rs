//! System calls for duplicating file descriptors (`dup`, `dup2`, `dup3`).

use super::*;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult};

/// `sys_dup` (SYS_DUP = 32)
/// Duplicate an open file descriptor.
#[wrap_syscall]
pub fn sys_dup(oldfd: i32) -> SyscallResult {
    if oldfd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(oldfd)?;
    let newfd = proc.fd_table.alloc(file);

    Ok(newfd as usize)
}

/// `sys_dup2` (SYS_DUP2 = 33)
/// Duplicate a file descriptor onto a specified target descriptor.
#[wrap_syscall]
pub fn sys_dup2(oldfd: i32, newfd: i32) -> SyscallResult {
    if oldfd < 0 || newfd < 0 {
        return Err(SyscallError::EBADF);
    }
    if oldfd == newfd {
        return Ok(newfd as usize);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(oldfd)?;
    proc.fd_table.set_with_flags(newfd, file, 0)?;

    Ok(newfd as usize)
}

/// `sys_dup3` (SYS_DUP3 = 292)
/// Duplicate a file descriptor with flags.
#[wrap_syscall]
pub fn sys_dup3(oldfd: i32, newfd: i32, flags: u32) -> SyscallResult {
    if oldfd == newfd || oldfd < 0 || newfd < 0 {
        return Err(SyscallError::EINVAL);
    }

    let cloexec = if (flags & O_CLOEXEC) != 0 {
        crate::fs::fd::FD_CLOEXEC
    } else {
        0
    };

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(oldfd)?;
    proc.fd_table.set_with_flags(newfd, file, cloexec)?;

    Ok(newfd as usize)
}
