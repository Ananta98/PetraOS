//! System calls for querying file system statistics (`statfs`, `fstatfs`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::vfs::types::StatFs;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr, UserPtr};

pub const RAMFS_MAGIC: i64 = 0x858458f6;

pub(crate) fn make_statfs() -> StatFs {
    StatFs {
        f_type: RAMFS_MAGIC,
        f_bsize: 4096,
        f_blocks: 262144, // ~1GB
        f_bfree: 200000,
        f_bavail: 200000,
        f_files: 65536,
        f_ffree: 60000,
        f_fsid: [0, 0],
        f_namelen: 255,
        f_frsize: 4096,
        f_flags: 0,
        f_spare: [0; 4],
    }
}

/// `sys_statfs` (SYS_STATFS = 137)
/// Get filesystem statistics by pathname.
pub fn sys_statfs(frame: &mut SyscallFrame) -> SyscallResult {
    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let buf_ptr = UserPtr::<StatFs>::from_u64(frame.arg2());

    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let _dentry = crate::fs::resolve_path(&full_path)?;

    let statfs = make_statfs();
    buf_ptr.write(statfs).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}

/// `sys_fstatfs` (SYS_FSTATFS = 138)
/// Get filesystem statistics by open file descriptor.
pub fn sys_fstatfs(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let buf_ptr = UserPtr::<StatFs>::from_u64(frame.arg2());

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let _file = proc.fd_table.get(fd)?;
    drop(proc);

    let statfs = make_statfs();
    buf_ptr.write(statfs).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}
