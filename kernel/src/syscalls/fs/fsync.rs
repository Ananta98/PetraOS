//! System calls for synchronizing file data with storage (`fsync`, `fdatasync`).

use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult};

/// `sys_fsync` (SYS_FSYNC = 74)
/// Synchronize a file's in-core state with storage device.
#[wrap_syscall]
pub fn sys_fsync(fd: i32) -> SyscallResult {
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);
    file.ops.sync()?;
    Ok(0)
}

/// `sys_fdatasync` (SYS_FDATASYNC = 75)
/// Synchronize a file's in-core data with storage device.
#[wrap_syscall]
pub fn sys_fdatasync(fd: i32) -> SyscallResult {
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);
    file.ops.sync()?;
    Ok(0)
}
