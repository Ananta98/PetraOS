//! sys_utimensat system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_utimensat` (SYS_UTIMENSAT = 280)
pub fn sys_utimensat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_cstr = UserCStr::from_u64(frame.arg2());
    let times_ptr = UserPtr::<LinuxTimespec>::from_u64(frame.arg3());
    let flags = frame.arg4() as i32;

    if path_cstr.is_null() || (path_cstr.as_u64() == 0) {
        if dfd < 0 {
            return Err(SyscallError::EBADF);
        }
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let creds = { Arc::clone(&proc_arc.lock().creds) };
        let proc = proc_arc.lock();
        let file = proc.fd_table.get(dfd)?;
        drop(proc);
        let st = file.ops.stat().or_else(|_| file.dentry.inode.ops.stat())?;

        if creds.euid != 0 && creds.euid != st.uid {
            return Err(SyscallError::EPERM);
        }

        let (atime, mtime) = read_utimens(times_ptr, st.atime, st.mtime)?;
        file.ops
            .utimens(atime, mtime)
            .or_else(|_| file.dentry.inode.ops.utimens(atime, mtime))?;
        return Ok(0);
    }

    let path = path_cstr.to_string(256)?;
    if path.is_empty() && (flags & crate::fs::AT_EMPTY_PATH) != 0 {
        if dfd < 0 {
            return Err(SyscallError::EBADF);
        }
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let creds = { Arc::clone(&proc_arc.lock().creds) };
        let proc = proc_arc.lock();
        let file = proc.fd_table.get(dfd)?;
        drop(proc);
        let st = file.ops.stat().or_else(|_| file.dentry.inode.ops.stat())?;

        if creds.euid != 0 && creds.euid != st.uid {
            return Err(SyscallError::EPERM);
        }

        let (atime, mtime) = read_utimens(times_ptr, st.atime, st.mtime)?;
        file.ops
            .utimens(atime, mtime)
            .or_else(|_| file.dentry.inode.ops.utimens(atime, mtime))?;
        return Ok(0);
    }

    let full_path = resolve_at_path(dfd, &path)?;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let creds = { Arc::clone(&proc_arc.lock().creds) };
    let st = crate::fs::stat(&full_path)?;

    if creds.euid != 0 && creds.euid != st.uid {
        return Err(SyscallError::EPERM);
    }

    let (atime, mtime) = read_utimens(times_ptr, st.atime, st.mtime)?;
    crate::fs::utimens(&full_path, atime, mtime)?;
    Ok(0)
}
