//! sys_gettimeofday system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;


/// `sys_gettimeofday` (SYS_GETTIMEOFDAY = 96)
/// Returns system wall-clock time in seconds and microseconds since Unix epoch.
pub fn sys_gettimeofday(frame: &mut SyscallFrame) -> SyscallResult {
    let tv_ptr = UserPtr::<TimeVal>::from_u64(frame.arg1());
    let tz_ptr = UserPtr::<TimeZone>::from_u64(frame.arg2());

    if !tv_ptr.is_null() {
        let (sec, usec) = crate::drivers::time::cmos_rtc::get_wall_time();
        let tv = TimeVal {
            tv_sec: sec as i64,
            tv_usec: usec as i64,
        };
        tv_ptr.write(tv).ok_or(SyscallError::EFAULT)?;
    }

    if !tz_ptr.is_null() {
        let tz = TimeZone {
            tz_minuteswest: 0,
            tz_dsttime: 0,
        };
        tz_ptr.write(tz).ok_or(SyscallError::EFAULT)?;
    }

    Ok(0)
}
