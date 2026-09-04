//! sys_lseek system call handler.

use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::vfs::types::SeekWhence;


/// `sys_lseek` (SYS_LSEEK = 8)
/// Reposition read/write file offset.
pub fn sys_lseek(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let offset = frame.arg2() as i64;
    let whence_raw = frame.arg3() as i32;

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
