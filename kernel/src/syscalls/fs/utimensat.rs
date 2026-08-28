//! System calls for updating file timestamps (`futimesat`, `utimensat`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr, UserPtr};
use alloc::sync::Arc;

/// `utimensat` special `tv_nsec` value: set the timestamp to the current time.
pub const UTIME_NOW: i64 = 0x3FFF_FFFF;
/// `utimensat` special `tv_nsec` value: leave the corresponding timestamp unchanged.
pub const UTIME_OMIT: i64 = 0x3FFF_FFFE;
/// Maximum valid nanosecond value within a timespec.
pub const NSEC_MAX: i64 = 999_999_999;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxTimespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

/// Current wall-clock time in seconds since the UNIX epoch.
pub(crate) fn wall_now_secs() -> u64 {
    crate::drivers::time::cmos_rtc::get_wall_time().0
}

/// Resolve one user-provided timespec against the value currently stored on disk.
pub(crate) fn resolve_timespec(ts: LinuxTimespec, current: u64) -> Result<u64, SyscallError> {
    match ts.tv_nsec {
        UTIME_NOW => Ok(wall_now_secs()),
        UTIME_OMIT => Ok(current),
        n if (0..=NSEC_MAX).contains(&n) => Ok(ts.tv_sec as u64),
        _ => Err(SyscallError::EINVAL),
    }
}

/// Convert a user-provided timespec pair into concrete `(atime, mtime)`
/// seconds for `utimensat`. A null pointer selects "current time" for both
/// fields; `UTIME_OMIT` fields fall back to the current on-disk values.
pub(crate) fn read_utimens(
    times_ptr: UserPtr<LinuxTimespec>,
    cur_atime: u64,
    cur_mtime: u64,
) -> Result<(u64, u64), SyscallError> {
    if times_ptr.is_null() {
        let now = wall_now_secs();
        return Ok((now, now));
    }
    let times = times_ptr.as_slice(2).ok_or(SyscallError::EFAULT)?;
    Ok((
        resolve_timespec(times[0], cur_atime)?,
        resolve_timespec(times[1], cur_mtime)?,
    ))
}

/// `sys_futimesat` (SYS_FUTIMESAT = 261)
/// Change timestamps of a file relative to a directory file descriptor.
pub fn sys_futimesat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_cstr = UserCStr::from_u64(frame.arg2());
    let utimes_ptr = UserPtr::<LinuxTimespec>::from_u64(frame.arg3());

    if path_cstr.is_null() {
        return Ok(0);
    }
    let path = path_cstr.to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    let (atime, mtime) = if !utimes_ptr.is_null() {
        let times = utimes_ptr.as_slice(2).ok_or(SyscallError::EFAULT)?;
        (
            resolve_timespec(times[0], 0)?,
            resolve_timespec(times[1], 0)?,
        )
    } else {
        // POSIX: a null timeval selects the current time for both fields.
        let now = wall_now_secs();
        (now, now)
    };
    crate::fs::utimens(&full_path, atime, mtime)?;
    Ok(0)
}

/// `sys_utimensat` (SYS_UTIMENSAT = 280)
/// Change file timestamps with nanosecond precision.
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
