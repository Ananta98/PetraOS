//! sys_isatty system call handler.

use crate::fs::vfs::types::VfsError::BadFd;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult};

/// `sys_isatty` (SYS_ISATTY = 215)
/// Test whether a file descriptor refers to a terminal.
#[wrap_syscall]
pub fn sys_isatty(fd: i32) -> SyscallResult {
    match crate::drivers::tty::isatty(fd) {
        Ok(true) => Ok(1),
        Ok(false) => Err(SyscallError::ENOTTY),
        Err(BadFd) => Err(SyscallError::EBADF),
        Err(_) => Err(SyscallError::ENOTTY),
    }
}
