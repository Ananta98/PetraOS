//! sys_isatty system call handler.

use crate::syscalls::{SyscallError, SyscallResult};
use crate::fs::vfs::types::VfsError::BadFd;
use crate::arch::syscall::syscall::SyscallFrame;


/// `sys_isatty` (SYS_ISATTY = 215)
/// Test whether a file descriptor refers to a terminal.
pub fn sys_isatty(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    match crate::tty::isatty(fd) {
        Ok(true) => Ok(1),
        Ok(false) => Err(SyscallError::ENOTTY),
        Err(BadFd) => Err(SyscallError::EBADF),
        Err(_) => Err(SyscallError::ENOTTY),
    }
}
