//! sys_getcwd system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_getcwd` (SYS_GETCWD = 79)
/// Get current working directory string.
pub fn sys_getcwd(frame: &mut SyscallFrame) -> SyscallResult {
    let buf = UserPtr::<u8>::from_u64(frame.arg1());
    let size = frame.arg2() as usize;

    if size == 0 || !buf.is_valid_for(size) {
        return Err(SyscallError::EINVAL);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let cwd_bytes = proc.cwd.as_bytes();
    if cwd_bytes.len() + 1 > size {
        return Err(SyscallError::ENOMEM);
    }

    buf.write_slice(cwd_bytes).ok_or(SyscallError::EFAULT)?;
    buf.offset(cwd_bytes.len())
        .write(0)
        .ok_or(SyscallError::EFAULT)?;

    Ok(buf.as_u64() as usize)
}
