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
    let buf = frame.arg2() as *const u8;
    let count = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    if (fd == 1 || fd == 2) && !buf.is_null() && count > 0 {
        // SAFETY: The user pointer `buf` and `count` are passed from active user process.
        let slice = unsafe { core::slice::from_raw_parts(buf, count) };
        if let Ok(s) = core::str::from_utf8(slice) {
            log::info!("[Userspace Output] {}", s.trim_end());
        }
    }

    Ok(count)
}

