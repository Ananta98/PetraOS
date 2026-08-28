//! sys_getrusage system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::syscalls::time::TimeVal;

/// Linux `struct rusage` layout for x86_64.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxRusage {
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

/// `sys_getrusage` (SYS_GETRUSAGE = 98)
/// Return resource usage measures for self, children, or thread.
pub fn sys_getrusage(frame: &mut SyscallFrame) -> SyscallResult {
    let who = frame.arg1() as i32;
    let rusage_ptr = UserPtr::<LinuxRusage>::from_u64(frame.arg2());

    if who != 0 && who != -1 && who != 1 {
        // RUSAGE_SELF = 0, RUSAGE_CHILDREN = -1, RUSAGE_THREAD = 1
        return Err(SyscallError::EINVAL);
    }

    let rusage = LinuxRusage::default();
    rusage_ptr.write(rusage).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}
