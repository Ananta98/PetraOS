//! System calls for reading from file descriptors (`read`, `pread64`, `readv`).

use super::*;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr, VirtAddr};

/// `sys_read` (SYS_READ = 0)
/// Read from a file descriptor.
#[wrap_syscall]
pub fn sys_read(fd: i32, buf: UserPtr<u8>, count: usize) -> SyscallResult {
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if count == 0 {
        return Ok(0);
    }
    let user_slice = buf.as_slice_mut(count).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let bytes_read = file.read(user_slice)?;
    Ok(bytes_read)
}

/// `sys_pread64` (SYS_PREAD64 = 17)
/// Read from a file descriptor at a specified offset without changing the file position.
#[wrap_syscall]
pub fn sys_pread64(fd: i32, buf: UserPtr<u8>, count: usize, offset: usize) -> SyscallResult {
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if count == 0 {
        return Ok(0);
    }
    let user_slice = buf.as_slice_mut(count).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let bytes_read = file.pread(user_slice, offset)?;
    Ok(bytes_read)
}

/// `sys_readv` (SYS_READV = 19)
/// Read data into multiple buffers (scatter/gather I/O).
#[wrap_syscall]
pub fn sys_readv(fd: i32, iov: VirtAddr, iovcnt: usize) -> SyscallResult {
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if iovcnt == 0 {
        return Ok(0);
    }
    if iovcnt > 1024 {
        return Err(SyscallError::EFAULT);
    }
    let iov_ptr = UserPtr::<IoVec>::new(iov);
    let iov_slice = iov_ptr.as_slice(iovcnt).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let mut total_read = 0;
    for iov in iov_slice {
        if iov.iov_len == 0 {
            continue;
        }
        let base_ptr = UserPtr::<u8>::from_u64(iov.iov_base);
        let user_slice = base_ptr
            .as_slice_mut(iov.iov_len)
            .ok_or(SyscallError::EFAULT)?;
        let n = file.read(user_slice)?;
        total_read += n;
        if n < iov.iov_len {
            break;
        }
    }
    Ok(total_read)
}


