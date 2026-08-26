//! sys_readv system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_readv` (SYS_READV = 19)
pub fn sys_readv(frame: &mut SyscallFrame) -> SyscallResult {
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
