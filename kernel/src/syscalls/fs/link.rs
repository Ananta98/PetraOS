//! System calls for hard links and symbolic links (`link`, `linkat`, `symlink`, `symlinkat`, `readlink`, `readlinkat`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr, UserPtr};

/// `sys_link` (SYS_LINK = 86)
/// Create a hard link to an existing file.
pub fn sys_link(frame: &mut SyscallFrame) -> SyscallResult {
    let old_path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let new_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let old_full = resolve_at_path(AT_FDCWD, &old_path)?;
    let new_full = resolve_at_path(AT_FDCWD, &new_path)?;
    crate::fs::link(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_linkat` (SYS_LINKAT = 265)
/// Create a hard link relative to directory file descriptors.
pub fn sys_linkat(frame: &mut SyscallFrame) -> SyscallResult {
    let olddfd = frame.arg1() as i32;
    let newdfd = frame.arg3() as i32;
    let _flags = frame.arg5() as i32;

    let old_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let new_path = UserCStr::from_u64(frame.arg4()).to_string(256)?;
    let old_full = resolve_at_path(olddfd, &old_path)?;
    let new_full = resolve_at_path(newdfd, &new_path)?;
    crate::fs::link(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_symlink` (SYS_SYMLINK = 88)
/// Create a symbolic link.
pub fn sys_symlink(frame: &mut SyscallFrame) -> SyscallResult {
    let target = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let link_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &link_path)?;
    crate::fs::symlink(&full_path, &target)?;
    Ok(0)
}

/// `sys_symlinkat` (SYS_SYMLINKAT = 266)
/// Create a symbolic link relative to a directory file descriptor.
pub fn sys_symlinkat(frame: &mut SyscallFrame) -> SyscallResult {
    let newdfd = frame.arg2() as i32;

    let target = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let link_path = UserCStr::from_u64(frame.arg3()).to_string(256)?;
    let full_path = resolve_at_path(newdfd, &link_path)?;
    crate::fs::symlink(&full_path, &target)?;
    Ok(0)
}

/// `sys_readlink` (SYS_READLINK = 89)
/// Read value of a symbolic link.
pub fn sys_readlink(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = UserCStr::from_u64(frame.arg1());
    let buf = UserPtr::<u8>::from_u64(frame.arg2());
    let bufsiz = frame.arg3() as usize;

    if bufsiz == 0 || !buf.is_valid_for(bufsiz) {
        return Err(SyscallError::EINVAL);
    }

    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let target = crate::fs::readlink(&full_path)?;
    let target_bytes = target.as_bytes();
    let copy_len = core::cmp::min(target_bytes.len(), bufsiz);

    let user_slice = buf.as_slice_mut(copy_len).ok_or(SyscallError::EFAULT)?;
    user_slice.copy_from_slice(&target_bytes[..copy_len]);

    Ok(copy_len)
}

/// `sys_readlinkat` (SYS_READLINKAT = 267)
/// Read value of a symbolic link relative to a directory file descriptor.
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
