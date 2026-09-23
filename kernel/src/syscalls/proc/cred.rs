//! Process credentials, user/group IDs, and process/session identities.
//!
//! Handles:
//! - Process IDs & Session: `getpid`, `getppid`, `getpgrp`, `setpgid`, `setsid`
//! - User & Group IDs: `getuid`, `getgid`, `setuid`, `setgid`, `geteuid`, `getegid`,
//!   `setreuid`, `setregid`, `setresuid`, `getresuid`, `setresgid`, `getresgid`,
//!   `setgroups`, `getgroups`, `setfsuid`, `setfsgid`

use crate::arch::syscall::syscall::SyscallFrame;
use crate::proc::ProcessId;
use crate::proc::process::credentials::{Credentials, MAX_SUPPLEMENTARY_GROUPS};
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use alloc::sync::Arc;

fn map_cred_err(err: &'static str) -> SyscallError {
    if err.starts_with("EINVAL") {
        SyscallError::EINVAL
    } else {
        SyscallError::EPERM
    }
}

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
    proc.sid = proc.pid;
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
/// POSIX `setuid`: privileged sets ruid/euid/suid/fsuid, unprivileged may
/// only switch `euid` between `uid` and `suid`, else `EPERM`.
pub fn sys_setuid(frame: &mut SyscallFrame) -> SyscallResult {
    let uid = frame.arg1() as u32;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = Arc::make_mut(&mut proc.creds);
    creds.set_uid(uid).map_err(map_cred_err)?;
    Ok(0)
}

/// `sys_setgid` (SYS_SETGID = 106)
/// POSIX `setgid` mirror of `setuid`.
pub fn sys_setgid(frame: &mut SyscallFrame) -> SyscallResult {
    let gid = frame.arg1() as u32;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = Arc::make_mut(&mut proc.creds);
    creds.set_gid(gid).map_err(map_cred_err)?;
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
/// Get list of supplementary group IDs (Linux semantics).
pub fn sys_getgroups(frame: &mut SyscallFrame) -> SyscallResult {
    let size = frame.arg1() as usize;
    let list_ptr = UserPtr::<u32>::from_u64(frame.arg2());

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let count = proc.creds.supplementary_gids.len();
    if size == 0 {
        return Ok(count);
    }
    if size < count {
        return Err(SyscallError::EINVAL);
    }
    if count == 0 {
        return Ok(0);
    }
    let mut buf = [0u32; MAX_SUPPLEMENTARY_GROUPS];
    for (i, g) in proc.creds.supplementary_gids.iter().enumerate() {
        if i >= count {
            break;
        }
        buf[i] = *g;
    }
    drop(proc);

    list_ptr
        .write_slice(&buf[..count])
        .ok_or(SyscallError::EFAULT)?;
    Ok(count)
}

/// `sys_setreuid` (SYS_SETREUID = 113)
/// Set real and/or effective user ID (`-1` means no change).
pub fn sys_setreuid(frame: &mut SyscallFrame) -> SyscallResult {
    let ruid = Credentials::opt_id(frame.arg1());
    let euid = Credentials::opt_id(frame.arg2());
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = Arc::make_mut(&mut proc.creds);
    creds.set_reuid(ruid, euid).map_err(map_cred_err)?;
    Ok(0)
}

/// `sys_setregid` (SYS_SETREGID = 114)
/// Set real and/or effective group ID (`-1` means no change).
pub fn sys_setregid(frame: &mut SyscallFrame) -> SyscallResult {
    let rgid = Credentials::opt_id(frame.arg1());
    let egid = Credentials::opt_id(frame.arg2());
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = Arc::make_mut(&mut proc.creds);
    creds.set_regid(rgid, egid).map_err(map_cred_err)?;
    Ok(0)
}

/// `sys_setgroups` (SYS_SETGROUPS = 116)
/// Set supplementary group list (root only).
pub fn sys_setgroups(frame: &mut SyscallFrame) -> SyscallResult {
    let size = frame.arg1() as usize;
    let list_ptr = UserPtr::<u32>::from_u64(frame.arg2());

    if size > MAX_SUPPLEMENTARY_GROUPS {
        return Err(SyscallError::EINVAL);
    }
    if size == 0 {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let mut proc = proc_arc.lock();
        let creds = Arc::make_mut(&mut proc.creds);
        creds.set_supplementary_gids(&[]).map_err(map_cred_err)?;
        return Ok(0);
    }
    if list_ptr.is_null() {
        return Err(SyscallError::EFAULT);
    }
    let mut buf = [0u32; MAX_SUPPLEMENTARY_GROUPS];
    list_ptr
        .read_slice(&mut buf[..size])
        .ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = Arc::make_mut(&mut proc.creds);
    creds
        .set_supplementary_gids(&buf[..size])
        .map_err(map_cred_err)?;
    Ok(0)
}

/// `sys_setresuid` (SYS_SETRESUID = 117)
/// Set real, effective and saved user IDs (`-1` means no change).
pub fn sys_setresuid(frame: &mut SyscallFrame) -> SyscallResult {
    let ruid = Credentials::opt_id(frame.arg1());
    let euid = Credentials::opt_id(frame.arg2());
    let suid = Credentials::opt_id(frame.arg3());
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = Arc::make_mut(&mut proc.creds);
    creds
        .set_resuid(ruid, euid, suid)
        .map_err(map_cred_err)?;
    Ok(0)
}

/// `sys_getresuid` (SYS_GETRESUID = 118)
/// Get real, effective and saved user IDs.
pub fn sys_getresuid(frame: &mut SyscallFrame) -> SyscallResult {
    let ruid_ptr = UserPtr::<u32>::from_u64(frame.arg1());
    let euid_ptr = UserPtr::<u32>::from_u64(frame.arg2());
    let suid_ptr = UserPtr::<u32>::from_u64(frame.arg3());

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let (ruid, euid, suid) = (proc.creds.uid, proc.creds.euid, proc.creds.suid);
    drop(proc);

    if !ruid_ptr.is_null() {
        ruid_ptr.write(ruid).ok_or(SyscallError::EFAULT)?;
    }
    if !euid_ptr.is_null() {
        euid_ptr.write(euid).ok_or(SyscallError::EFAULT)?;
    }
    if !suid_ptr.is_null() {
        suid_ptr.write(suid).ok_or(SyscallError::EFAULT)?;
    }
    Ok(0)
}

/// `sys_setresgid` (SYS_SETRESGID = 119)
/// Set real, effective and saved group IDs (`-1` means no change).
pub fn sys_setresgid(frame: &mut SyscallFrame) -> SyscallResult {
    let rgid = Credentials::opt_id(frame.arg1());
    let egid = Credentials::opt_id(frame.arg2());
    let sgid = Credentials::opt_id(frame.arg3());
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = Arc::make_mut(&mut proc.creds);
    creds
        .set_resgid(rgid, egid, sgid)
        .map_err(map_cred_err)?;
    Ok(0)
}

/// `sys_getresgid` (SYS_GETRESGID = 120)
/// Get real, effective and saved group IDs.
pub fn sys_getresgid(frame: &mut SyscallFrame) -> SyscallResult {
    let rgid_ptr = UserPtr::<u32>::from_u64(frame.arg1());
    let egid_ptr = UserPtr::<u32>::from_u64(frame.arg2());
    let sgid_ptr = UserPtr::<u32>::from_u64(frame.arg3());

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let (rgid, egid, sgid) = (proc.creds.gid, proc.creds.egid, proc.creds.sgid);
    drop(proc);

    if !rgid_ptr.is_null() {
        rgid_ptr.write(rgid).ok_or(SyscallError::EFAULT)?;
    }
    if !egid_ptr.is_null() {
        egid_ptr.write(egid).ok_or(SyscallError::EFAULT)?;
    }
    if !sgid_ptr.is_null() {
        sgid_ptr.write(sgid).ok_or(SyscallError::EFAULT)?;
    }
    Ok(0)
}

/// `sys_setfsuid` (SYS_SETFSUID = 122)
/// Set filesystem user ID, returns previous value.
pub fn sys_setfsuid(frame: &mut SyscallFrame) -> SyscallResult {
    let uid = frame.arg1() as u32;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = Arc::make_mut(&mut proc.creds);
    let prev = creds.set_fsuid(uid).map_err(map_cred_err)?;
    Ok(prev as usize)
}

/// `sys_setfsgid` (SYS_SETFSGID = 123)
/// Set filesystem group ID, returns previous value.
pub fn sys_setfsgid(frame: &mut SyscallFrame) -> SyscallResult {
    let gid = frame.arg1() as u32;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = Arc::make_mut(&mut proc.creds);
    let prev = creds.set_fsgid(gid).map_err(map_cred_err)?;
    Ok(prev as usize)
}

/// `sys_getpgid` (SYS_GETPGID = 121)
/// Get process group ID of specified process (0 means calling process).
pub fn sys_getpgid(frame: &mut SyscallFrame) -> SyscallResult {
    let pid_raw = frame.arg1() as i32;
    if pid_raw < 0 {
        return Err(SyscallError::EINVAL);
    }
    if pid_raw == 0 {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let proc = proc_arc.lock();
        return Ok(proc.pgid.as_u64() as usize);
    }
    let target_pid = ProcessId(pid_raw as u64);
    let target_proc = crate::proc::find_process(target_pid).ok_or(SyscallError::ESRCH)?;
    let proc = target_proc.lock();
    Ok(proc.pgid.as_u64() as usize)
}

/// `sys_getsid` (SYS_GETSID = 124)
/// Get session ID of specified process (0 means calling process).
pub fn sys_getsid(frame: &mut SyscallFrame) -> SyscallResult {
    let pid_raw = frame.arg1() as i32;
    if pid_raw < 0 {
        return Err(SyscallError::EINVAL);
    }
    if pid_raw == 0 {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let proc = proc_arc.lock();
        return Ok(proc.sid.as_u64() as usize);
    }
    let target_pid = ProcessId(pid_raw as u64);
    let target_proc = crate::proc::find_process(target_pid).ok_or(SyscallError::ESRCH)?;
    let proc = target_proc.lock();
    Ok(proc.sid.as_u64() as usize)
}
