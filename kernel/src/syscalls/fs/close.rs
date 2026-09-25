//! sys_close system call handler.

use crate::syscalls::{SyscallError, SyscallResult, wrap_syscall};

/// `sys_close` (SYS_CLOSE = 3)
/// Close a file descriptor.
#[wrap_syscall]
pub fn sys_close(fd: i32) -> SyscallResult {
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    proc.fd_table.close(fd)?;

    Ok(0)
}
