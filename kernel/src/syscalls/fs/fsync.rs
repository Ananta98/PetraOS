//! System calls for synchronizing file data with storage (`fsync`, `fdatasync`).

use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult};

/// `sys_fsync` (SYS_FSYNC = 74)
/// Synchronize a file's in-core state with storage device.
pub fn sys_fsync(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
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
pub fn sys_fdatasync(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
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
