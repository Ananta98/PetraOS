//! sys_ioctl system call handler.

use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::vfs::types::VfsError::{BadFd, InvalidInput, NotSupported};
use crate::syscalls::{SyscallError, SyscallResult};

/// `sys_ioctl` (SYS_IOCTL = 16)
/// Control terminal and block devices.
pub fn sys_ioctl(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let cmd = frame.arg2() as u64;
    let arg = frame.arg3() as usize;

    crate::drivers::tty::do_ioctl(fd, cmd, arg).map_err(|e| match e {
        BadFd => SyscallError::EBADF,
        InvalidInput => SyscallError::EFAULT,
        NotSupported => SyscallError::ENOTTY,
        _ => SyscallError::EINVAL,
    })
}
