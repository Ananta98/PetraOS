//! sys_close system call handler.

use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;


/// `sys_close` (SYS_CLOSE = 3)
/// Close a file descriptor.
pub fn sys_close(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    proc.fd_table.close(fd)?;

    Ok(0)
}
