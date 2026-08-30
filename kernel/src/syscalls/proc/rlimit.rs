//! Resource limit and resource usage system calls (`getrlimit`, `setrlimit`, `prlimit64`, `getrusage`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::time::TimeVal;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};

/// Linux 64-bit resource limit structure.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RLimit64 {
    pub rlim_cur: u64,
    pub rlim_max: u64,
}

pub const RLIM_INFINITY: u64 = !0u64;

pub fn get_default_rlimit(resource: i32) -> RLimit64 {
    match resource {
        3 /* RLIMIT_STACK */ => RLimit64 {
            rlim_cur: 8 * 1024 * 1024,
            rlim_max: 64 * 1024 * 1024,
        },
        7 /* RLIMIT_NOFILE */ => RLimit64 {
            rlim_cur: 1024,
            rlim_max: 4096,
        },
        6 /* RLIMIT_NPROC */ => RLimit64 {
            rlim_cur: 4096,
            rlim_max: 4096,
        },
        _ => RLimit64 {
            rlim_cur: RLIM_INFINITY,
            rlim_max: RLIM_INFINITY,
        },
    }
}

/// Linux `struct rusage` layout for x86_64.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RUsage {
    pub ru_utime: TimeVal,
    pub ru_stime: TimeVal,
    pub ru_maxrss: i64,
    pub ru_ixrss: i64,
    pub ru_idrss: i64,
    pub ru_isrss: i64,
    pub ru_minflt: i64,
    pub ru_majflt: i64,
    pub ru_nswap: i64,
    pub ru_inblock: i64,
    pub ru_oublock: i64,
    pub ru_msgsnd: i64,
    pub ru_msgrcv: i64,
    pub ru_nsignals: i64,
    pub ru_nvcsw: i64,
    pub ru_nivcsw: i64,
}

/// Alias for Linux standard naming.
pub type LinuxRusage = RUsage;

/// `sys_getrlimit` (SYS_GETRLIMIT = 97)
/// Get resource limits.
pub fn sys_getrlimit(frame: &mut SyscallFrame) -> SyscallResult {
    let resource = frame.arg1() as i32;
    let rlim_ptr = UserPtr::<RLimit64>::from_u64(frame.arg2());

    let limit = get_default_rlimit(resource);
    rlim_ptr.write(limit).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}

/// `sys_setrlimit` (SYS_SETRLIMIT = 160)
/// Set resource limits.
pub fn sys_setrlimit(frame: &mut SyscallFrame) -> SyscallResult {
    let _resource = frame.arg1() as i32;
    let rlim_ptr = UserPtr::<RLimit64>::from_u64(frame.arg2());

    if !rlim_ptr.is_valid() {
        return Err(SyscallError::EFAULT);
    }
    Ok(0)
}

/// `sys_prlimit64` (SYS_PRLIMIT64 = 302)
/// Get/set resource limits of an arbitrary process.
pub fn sys_prlimit64(frame: &mut SyscallFrame) -> SyscallResult {
    let _pid = frame.arg1() as i32;
    let resource = frame.arg2() as i32;
    let new_limit_ptr = UserPtr::<RLimit64>::from_u64(frame.arg3());
    let old_limit_ptr = UserPtr::<RLimit64>::from_u64(frame.arg4());

    if !new_limit_ptr.is_null() && !new_limit_ptr.is_valid() {
        return Err(SyscallError::EFAULT);
    }

    if !old_limit_ptr.is_null() {
        let limit = get_default_rlimit(resource);
        old_limit_ptr.write(limit).ok_or(SyscallError::EFAULT)?;
    }

    Ok(0)
}

/// `sys_getrusage` (SYS_GETRUSAGE = 98)
/// Return resource usage measures for self, children, or thread.
pub fn sys_getrusage(frame: &mut SyscallFrame) -> SyscallResult {
    let who = frame.arg1() as i32;
    let rusage_ptr = UserPtr::<RUsage>::from_u64(frame.arg2());

    if who != 0 && who != -1 && who != 1 {
        // RUSAGE_SELF = 0, RUSAGE_CHILDREN = -1, RUSAGE_THREAD = 1
        return Err(SyscallError::EINVAL);
    }

    let rusage = RUsage::default();
    rusage_ptr.write(rusage).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}
