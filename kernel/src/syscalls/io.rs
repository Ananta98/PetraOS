use crate::arch::syscall::syscall::SyscallFrame;
use super::{SyscallError, SyscallResult};

/// `sys_read` (SYS_READ = 0)
/// Read from a file descriptor.
pub fn sys_read(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let _buf = frame.arg2() as *mut u8;
    let _count = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    Ok(0)
}

/// `sys_write` (SYS_WRITE = 1)
/// Write to a file descriptor.
pub fn sys_write(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let _buf = frame.arg2() as *const u8;
    let count = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    // STDOUT (1) / STDERR (2) fallback output
    Ok(count)
}
