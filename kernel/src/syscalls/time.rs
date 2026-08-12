use super::{is_user_ptr_valid, SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;

/// POSIX timeval structure for `sys_gettimeofday`
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TimeVal {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

/// POSIX timezone structure for `sys_gettimeofday`
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TimeZone {
    pub tz_minuteswest: i32,
    pub tz_dsttime: i32,
}

/// POSIX tms structure for `sys_times`
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Tms {
    pub tms_utime: i64,  // User CPU time in clock ticks
    pub tms_stime: i64,  // System CPU time in clock ticks
    pub tms_cutime: i64, // User CPU time of dead children
    pub tms_cstime: i64, // System CPU time of dead children
}

/// `sys_gettimeofday` (SYS_GETTIMEOFDAY = 96)
/// Returns system wall-clock time in seconds and microseconds since Unix epoch.
pub fn sys_gettimeofday(frame: &mut SyscallFrame) -> SyscallResult {
    let tv_ptr = frame.arg1() as *mut TimeVal;
    let tz_ptr = frame.arg2() as *mut TimeZone;

    if !tv_ptr.is_null() {
        if !is_user_ptr_valid(tv_ptr as u64, core::mem::size_of::<TimeVal>()) {
            return Err(SyscallError::EFAULT);
        }
        let (sec, usec) = crate::drivers::time::cmos_rtc::get_wall_time();
        let tv = TimeVal {
            tv_sec: sec as i64,
            tv_usec: usec as i64,
        };
        // SAFETY: tv_ptr verified with is_user_ptr_valid above.
        unsafe {
            core::ptr::write_volatile(tv_ptr, tv);
        }
    }

    if !tz_ptr.is_null() {
        if !is_user_ptr_valid(tz_ptr as u64, core::mem::size_of::<TimeZone>()) {
            return Err(SyscallError::EFAULT);
        }
        let tz = TimeZone {
            tz_minuteswest: 0,
            tz_dsttime: 0,
        };
        // SAFETY: tz_ptr verified with is_user_ptr_valid above.
        unsafe {
            core::ptr::write_volatile(tz_ptr, tz);
        }
    }

    Ok(0)
}

/// `sys_times` (SYS_TIMES = 100)
/// Returns elapsed system clock ticks since system boot, and fills process CPU timing.
pub fn sys_times(frame: &mut SyscallFrame) -> SyscallResult {
    let buf_ptr = frame.arg1() as *mut Tms;

    // Standard POSIX clock ticks per second (CLK_TCK = 100)
    let elapsed_ns = crate::arch::timer::hpet::elapsed_ns();
    let total_ticks = (elapsed_ns / 10_000_000) as i64; // 10ms per tick (100Hz)

    if !buf_ptr.is_null() {
        if !is_user_ptr_valid(buf_ptr as u64, core::mem::size_of::<Tms>()) {
            return Err(SyscallError::EFAULT);
        }

        // Retrieve current process CPU times if available
        let mut utime = total_ticks;
        let mut stime = 0i64;

        if let Some(proc_arc) = crate::proc::current_process() {
            let proc = proc_arc.lock();
            // Estimate process CPU ticks from threads' accumulated vruntime
            let mut process_vruntime_ns = 0u64;
            for thread_arc in proc.threads.values() {
                let thread = thread_arc.lock();
                process_vruntime_ns = process_vruntime_ns.saturating_add(thread.vruntime);
            }
            if process_vruntime_ns > 0 {
                utime = (process_vruntime_ns / 10_000_000) as i64;
            }
        }

        let tms = Tms {
            tms_utime: utime,
            tms_stime: stime,
            tms_cutime: 0,
            tms_cstime: 0,
        };

        // SAFETY: buf_ptr verified with is_user_ptr_valid above.
        unsafe {
            core::ptr::write_volatile(buf_ptr, tms);
        }
    }

    Ok(total_ticks as usize)
}
