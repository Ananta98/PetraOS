//! sys_readlinkat system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_readlinkat` (SYS_READLINKAT = 267)
pub fn sys_readlinkat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = UserCStr::from_u64(frame.arg2());
    let buf = UserPtr::<u8>::from_u64(frame.arg3());
    let bufsiz = frame.arg4() as usize;

    if bufsiz == 0 || !buf.is_valid_for(bufsiz) {
        return Err(SyscallError::EINVAL);
    }

    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    let target = crate::fs::readlink(&full_path)?;
    let target_bytes = target.as_bytes();
    let copy_len = core::cmp::min(target_bytes.len(), bufsiz);

    let user_slice = buf.as_slice_mut(copy_len).ok_or(SyscallError::EFAULT)?;
    user_slice.copy_from_slice(&target_bytes[..copy_len]);

    Ok(copy_len)
}
