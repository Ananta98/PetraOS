//! sys_ioctl system call handler.

use crate::fs::vfs::types::VfsError::{BadFd, InvalidInput, NotSupported};
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult};

/// `sys_ioctl` (SYS_IOCTL = 16)
/// Control terminal and block devices.
#[wrap_syscall]
pub fn sys_ioctl(fd: i32, cmd: u64, arg: usize) -> SyscallResult {
    crate::drivers::tty::do_ioctl(fd, cmd, arg).map_err(|e| match e {
        BadFd => SyscallError::EBADF,
        InvalidInput => SyscallError::EFAULT,
        NotSupported => SyscallError::ENOTTY,
        _ => SyscallError::EINVAL,
    })
}
