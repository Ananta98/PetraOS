//! System calls for writing to file descriptors (`write`, `pwrite64`, `writev`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};

/// `sys_write` (SYS_WRITE = 1)
/// Write to a file descriptor.
pub fn sys_write(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let buf = UserPtr::<u8>::from_u64(frame.arg2());
    let count = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if count == 0 {
        return Ok(0);
    }
    let user_slice = buf.as_slice(count).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let bytes_written = file.write(user_slice)?;
    Ok(bytes_written)
}

/// `sys_pwrite64` (SYS_PWRITE64 = 18)
/// Write to a file descriptor at a specified offset without changing the file position.
pub fn sys_pwrite64(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let buf = UserPtr::<u8>::from_u64(frame.arg2());
    let count = frame.arg3() as usize;
    let offset = frame.arg4() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if count == 0 {
        return Ok(0);
    }
    let user_slice = buf.as_slice(count).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let bytes_written = file.pwrite(user_slice, offset)?;
    Ok(bytes_written)
}

/// `sys_writev` (SYS_WRITEV = 20)
/// Write data from multiple buffers (scatter/gather I/O).
pub fn sys_writev(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let iov_ptr = UserPtr::<IoVec>::from_u64(frame.arg2());
    let iovcnt = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if iovcnt == 0 {
        return Ok(0);
    }
    if iovcnt > 1024 {
        return Err(SyscallError::EFAULT);
    }
    let iov_slice = iov_ptr.as_slice(iovcnt).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let mut total_written = 0;
    for iov in iov_slice {
        if iov.iov_len == 0 {
            continue;
        }
        let base_ptr = UserPtr::<u8>::from_u64(iov.iov_base);
        let user_slice = base_ptr.as_slice(iov.iov_len).ok_or(SyscallError::EFAULT)?;
        let n = file.write(user_slice)?;
        total_written += n;
        if n < iov.iov_len {
            break;
        }
    }
    Ok(total_written)
}
