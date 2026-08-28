//! System calls for creating inter-process pipes (`pipe`, `pipe2`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};

/// `sys_pipe` (SYS_PIPE = 22)
/// Create an anonymous inter-process pipe.
pub fn sys_pipe(frame: &mut SyscallFrame) -> SyscallResult {
    let pipefd = UserPtr::<i32>::from_u64(frame.arg1());

    let (f_read, f_write) = crate::fs::pipefs::create_pipe(false)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let r_fd = proc.fd_table.alloc(f_read);
    let w_fd = proc.fd_table.alloc(f_write);

    pipefd.write(r_fd).ok_or(SyscallError::EFAULT)?;
    pipefd.offset(1).write(w_fd).ok_or(SyscallError::EFAULT)?;

    Ok(0)
}

/// `sys_pipe2` (SYS_PIPE2 = 293)
/// Create an anonymous pipe with flags.
pub fn sys_pipe2(frame: &mut SyscallFrame) -> SyscallResult {
    let pipefd = UserPtr::<i32>::from_u64(frame.arg1());
    let flags = frame.arg2() as u32;

    let nonblocking = (flags & O_NONBLOCK) != 0;
    let cloexec = if (flags & O_CLOEXEC) != 0 {
        crate::fs::fd::FD_CLOEXEC
    } else {
        0
    };

    let (f_read, f_write) = crate::fs::pipefs::create_pipe(nonblocking)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let r_fd = proc.fd_table.alloc_with_flags(f_read, cloexec);
    let w_fd = proc.fd_table.alloc_with_flags(f_write, cloexec);

    pipefd.write(r_fd).ok_or(SyscallError::EFAULT)?;
    pipefd.offset(1).write(w_fd).ok_or(SyscallError::EFAULT)?;

    Ok(0)
}
