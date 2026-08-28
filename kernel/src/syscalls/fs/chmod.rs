//! System calls for changing file mode bits / permissions (`chmod`, `fchmod`, `fchmodat`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr};
use alloc::sync::Arc;

/// `sys_chmod` (SYS_CHMOD = 90)
/// Change permissions of a file.
pub fn sys_chmod(frame: &mut SyscallFrame) -> SyscallResult {
    let mode = frame.arg2() as u32;

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
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
pub fn sys_fchmod(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let mode = frame.arg2() as u32;

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
pub fn sys_fchmodat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let mode = frame.arg3() as u32;
    let _flags = frame.arg4() as i32;

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    crate::fs::chmod(&full_path, mode)?;
    Ok(0)
}
