//! System calls for predeclaring file access patterns (`fadvise64`).

use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult};

/// `sys_fadvise64` (SYS_FADVISE64 = 221)
/// Predeclare an access pattern for file data.
#[wrap_syscall]
pub fn sys_fadvise64(fd: i32, _offset: i64, len: i64, advice: i32) -> SyscallResult {
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    if len < 0 {
        return Err(SyscallError::EINVAL);
    }

    // POSIX advice values (POSIX_FADV_NORMAL=0, RANDOM=1, SEQUENTIAL=2, WILLNEED=3, DONTNEED=4, NOREUSE=5)
    if !(0..=5).contains(&advice) {
        return Err(SyscallError::EINVAL);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let _file = proc.fd_table.get(fd)?;
    drop(proc);

    // Advisory hint acknowledged
    Ok(0)
}
