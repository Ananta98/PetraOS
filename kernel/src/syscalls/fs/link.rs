//! System calls for hard links and symbolic links (`link`, `linkat`, `symlink`, `symlinkat`, `readlink`, `readlinkat`).

use super::*;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserCStr, UserPtr};

/// `sys_link` (SYS_LINK = 86)
/// Create a hard link to an existing file.
#[wrap_syscall]
pub fn sys_link(old_path_ptr: UserCStr, new_path_ptr: UserCStr) -> SyscallResult {
    let old_path = old_path_ptr.to_string(256)?;
    let new_path = new_path_ptr.to_string(256)?;
    let old_full = resolve_at_path(AT_FDCWD, &old_path)?;
    let new_full = resolve_at_path(AT_FDCWD, &new_path)?;
    crate::fs::link(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_linkat` (SYS_LINKAT = 265)
/// Create a hard link relative to directory file descriptors.
#[wrap_syscall]
pub fn sys_linkat(
    olddfd: i32,
    old_path_ptr: UserCStr,
    newdfd: i32,
    new_path_ptr: UserCStr,
    _flags: i32,
) -> SyscallResult {
    let old_path = old_path_ptr.to_string(256)?;
    let new_path = new_path_ptr.to_string(256)?;
    let old_full = resolve_at_path(olddfd, &old_path)?;
    let new_full = resolve_at_path(newdfd, &new_path)?;
    crate::fs::link(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_symlink` (SYS_SYMLINK = 88)
/// Create a symbolic link.
#[wrap_syscall]
pub fn sys_symlink(target_ptr: UserCStr, link_path_ptr: UserCStr) -> SyscallResult {
    let target = target_ptr.to_string(256)?;
    let link_path = link_path_ptr.to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &link_path)?;
    crate::fs::symlink(&full_path, &target)?;
    Ok(0)
}

/// `sys_symlinkat` (SYS_SYMLINKAT = 266)
/// Create a symbolic link relative to a directory file descriptor.
#[wrap_syscall]
pub fn sys_symlinkat(target_ptr: UserCStr, newdfd: i32, link_path_ptr: UserCStr) -> SyscallResult {
    let target = target_ptr.to_string(256)?;
    let link_path = link_path_ptr.to_string(256)?;
    let full_path = resolve_at_path(newdfd, &link_path)?;
    crate::fs::symlink(&full_path, &target)?;
    Ok(0)
}

/// `sys_readlink` (SYS_READLINK = 89)
/// Read value of a symbolic link.
#[wrap_syscall]
pub fn sys_readlink(path_ptr: UserCStr, buf: UserPtr<u8>, bufsiz: usize) -> SyscallResult {
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
#[wrap_syscall]
pub fn sys_readlinkat(
    dfd: i32,
    path_ptr: UserCStr,
    buf: UserPtr<u8>,
    bufsiz: usize,
) -> SyscallResult {
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
