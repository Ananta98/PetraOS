//! System calls for changing file mode bits / permissions (`chmod`, `fchmod`, `fchmodat`).

use super::*;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserCStr};
use alloc::sync::Arc;

/// `sys_chmod` (SYS_CHMOD = 90)
/// Change permissions of a file.
#[wrap_syscall]
pub fn sys_chmod(path_ptr: UserCStr, mode: u32) -> SyscallResult {
    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;

    let st = crate::fs::stat(&full_path)?;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let creds = { Arc::clone(&proc_arc.lock().creds) };
    if creds.euid != 0 && creds.euid != st.uid {
        return Err(SyscallError::EPERM);
    }

    crate::fs::chmod(&full_path, mode)?;
    Ok(0)
}

/// `sys_fchmod` (SYS_FCHMOD = 91)
/// Change permissions of an open file descriptor.
#[wrap_syscall]
pub fn sys_fchmod(fd: i32, mode: u32) -> SyscallResult {
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    let creds = Arc::clone(&proc.creds);
    drop(proc);

    let st = file.ops.stat().or_else(|_| file.dentry.inode.ops.stat())?;
    if creds.euid != 0 && creds.euid != st.uid {
        return Err(SyscallError::EPERM);
    }

    file.ops
        .chmod(mode)
        .or_else(|_| file.dentry.inode.ops.chmod(mode))?;
    Ok(0)
}

/// `sys_fchmodat` (SYS_FCHMODAT = 268)
/// Change permissions of a file relative to a directory file descriptor.
#[wrap_syscall]
pub fn sys_fchmodat(dfd: i32, path_ptr: UserCStr, mode: u32, _flags: i32) -> SyscallResult {
    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;

    let st = crate::fs::stat(&full_path)?;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let creds = { Arc::clone(&proc_arc.lock().creds) };
    if creds.euid != 0 && creds.euid != st.uid {
        return Err(SyscallError::EPERM);
    }

    crate::fs::chmod(&full_path, mode)?;
    Ok(0)
}
