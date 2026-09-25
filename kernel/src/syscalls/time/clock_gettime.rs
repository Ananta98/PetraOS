//! sys_clock_gettime system call handler.

use super::*;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_clock_gettime` (SYS_CLOCK_GETTIME = 228)
/// Retrieve time of the specified clock.
#[wrap_syscall]
pub fn sys_clock_gettime(clock_id: i32, tp_ptr: UserPtr<TimeSpec>) -> SyscallResult {
    let ts = match clock_id {
        CLOCK_REALTIME | CLOCK_REALTIME_COARSE => {
            let (sec, usec) = crate::drivers::time::cmos_rtc::get_wall_time();
            TimeSpec {
                tv_sec: sec as i64,
                tv_nsec: (usec as i64) * 1000,
            }
        }
        CLOCK_MONOTONIC | CLOCK_MONOTONIC_RAW | CLOCK_MONOTONIC_COARSE | CLOCK_BOOTTIME => {
            let elapsed_ns = crate::clock::elapsed_ns();
            TimeSpec {
                tv_sec: (elapsed_ns / 1_000_000_000) as i64,
                tv_nsec: (elapsed_ns % 1_000_000_000) as i64,
            }
        }
        CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            let elapsed_ns = crate::clock::elapsed_ns();
            TimeSpec {
                tv_sec: (elapsed_ns / 1_000_000_000) as i64,
                tv_nsec: (elapsed_ns % 1_000_000_000) as i64,
            }
        }
        _ => return Err(SyscallError::EINVAL),
    };

    tp_ptr.write(ts).ok_or(SyscallError::EFAULT)?;

    Ok(0)
}
