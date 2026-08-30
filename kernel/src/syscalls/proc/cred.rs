//! Process credentials, user/group IDs, and process/session identities.
//!
//! Handles:
//! - Process IDs & Session: `getpid`, `getppid`, `getpgrp`, `setpgid`, `setsid`
//! - User & Group IDs: `getuid`, `getgid`, `setuid`, `setgid`, `geteuid`, `getegid`, `getgroups`

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::proc::ProcessId;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use alloc::sync::Arc;

// ── Process IDs & Session Management ────────────────────────────────────────

/// `sys_getpid` (SYS_GETPID = 39)
/// Get process ID.
pub fn sys_getpid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.pid.as_u64() as usize)
}

/// `sys_getppid` (SYS_GETPPID = 110)
/// Get parent process ID.
pub fn sys_getppid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.ppid.as_u64() as usize)
}

/// `sys_getpgrp` (SYS_GETPGRP = 111)
/// Get process group ID.
pub fn sys_getpgrp(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.pgid.as_u64() as usize)
}

/// `sys_setpgid` (SYS_SETPGID = 109)
/// Set process group ID.
pub fn sys_setpgid(frame: &mut SyscallFrame) -> SyscallResult {
    let pid_raw = frame.arg1() as i32;
    let pgid_raw = frame.arg2() as i32;

    let target_pid = if pid_raw <= 0 {
        let current_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        current_arc.lock().pid
    } else {
        ProcessId(pid_raw as u64)
    };

    let target_proc = crate::proc::find_process(target_pid).ok_or(SyscallError::ESRCH)?;
    let mut proc = target_proc.lock();

    let new_pgid = if pgid_raw <= 0 {
        proc.pid
    } else {
        ProcessId(pgid_raw as u64)
    };

    proc.pgid = new_pgid;
    Ok(0)
}

/// `sys_setsid` (SYS_SETSID = 112)
/// Creates a new session if the calling process is not a process group leader.
pub fn sys_setsid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    proc.pgid = proc.pid;
    Ok(proc.pid.as_u64() as usize)
}

// ── User & Group Credentials ────────────────────────────────────────────────

/// `sys_getuid` (SYS_GETUID = 102)
/// Get real user ID.
pub fn sys_getuid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.creds.uid as usize)
}

/// `sys_getgid` (SYS_GETGID = 104)
/// Get real group ID.
pub fn sys_getgid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.creds.gid as usize)
}

/// `sys_setuid` (SYS_SETUID = 105)
/// Set user ID.
pub fn sys_setuid(frame: &mut SyscallFrame) -> SyscallResult {
    let uid = frame.arg1() as u32;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = Arc::make_mut(&mut proc.creds);
    creds.uid = uid;
    creds.euid = uid;
    Ok(0)
}

/// `sys_setgid` (SYS_SETGID = 106)
/// Set group ID.
pub fn sys_setgid(frame: &mut SyscallFrame) -> SyscallResult {
    let gid = frame.arg1() as u32;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = Arc::make_mut(&mut proc.creds);
    creds.gid = gid;
    creds.egid = gid;
    Ok(0)
}

/// `sys_geteuid` (SYS_GETEUID = 107)
/// Get effective user ID.
pub fn sys_geteuid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.creds.euid as usize)
}

/// `sys_getegid` (SYS_GETEGID = 108)
/// Get effective group ID.
pub fn sys_getegid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.creds.egid as usize)
}

/// `sys_getgroups` (SYS_GETGROUPS = 115)
/// Get list of supplementary group IDs.
pub fn sys_getgroups(frame: &mut SyscallFrame) -> SyscallResult {
    let size = frame.arg1() as i32;
    let list_ptr = UserPtr::<u32>::from_u64(frame.arg2());

    if size < 0 {
        return Err(SyscallError::EINVAL);
    }
    if size == 0 {
        return Ok(1);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let gid = proc.creds.gid;
    drop(proc);

    list_ptr.write(gid).ok_or(SyscallError::EFAULT)?;
    Ok(1)
}
