//! sys_lseek system call handler.

use crate::fs::vfs::types::SeekWhence;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult};

/// `sys_lseek` (SYS_LSEEK = 8)
/// Reposition read/write file offset.
#[wrap_syscall]
pub fn sys_lseek(fd: i32, offset: i64, whence_raw: i32) -> SyscallResult {
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let whence = match whence_raw {
        0 => SeekWhence::Set,
        1 => SeekWhence::Cur,
        2 => SeekWhence::End,
        _ => return Err(SyscallError::EINVAL),
    };

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let new_offset = match file.lseek(offset, whence) {
        Ok(off) => off,
        Err(crate::fs::vfs::types::VfsError::NotSupported) => return Err(SyscallError::ESPIPE),
        Err(e) => return Err(e.into()),
    };
    Ok(new_offset)
}
