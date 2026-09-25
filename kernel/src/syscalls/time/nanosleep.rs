//! sys_nanosleep system call handler.

use super::*;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_nanosleep` (SYS_NANOSLEEP = 35)
/// High-resolution sleep.
#[wrap_syscall]
pub fn sys_nanosleep(req_ptr: UserPtr<TimeSpec>, rem_ptr: UserPtr<TimeSpec>) -> SyscallResult {
    let req = req_ptr.read_unaligned().ok_or(SyscallError::EFAULT)?;
    if req.tv_sec < 0 || req.tv_nsec < 0 || req.tv_nsec >= 1_000_000_000 {
        return Err(SyscallError::EINVAL);
    }

    let target_ns = (req.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(req.tv_nsec as u64);

    let start_ns = crate::clock::elapsed_ns();
    while crate::clock::elapsed_ns().saturating_sub(start_ns) < target_ns {
        crate::proc::thread::Thread::yield_cpu();
    }

    if !rem_ptr.is_null() {
        let _ = rem_ptr.write(TimeSpec::default());
    }

    Ok(0)
}
