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
            core::ptr::write_unaligned(tv_ptr, tv);
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
            core::ptr::write_unaligned(tz_ptr, tz);
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
            core::ptr::write_unaligned(buf_ptr, tms);
        }
    }

    Ok(total_ticks as usize)
}

pub const CLOCK_REALTIME: i32 = 0;
pub const CLOCK_MONOTONIC: i32 = 1;
pub const CLOCK_PROCESS_CPUTIME_ID: i32 = 2;
pub const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
pub const CLOCK_MONOTONIC_RAW: i32 = 4;
pub const CLOCK_REALTIME_COARSE: i32 = 5;
pub const CLOCK_MONOTONIC_COARSE: i32 = 6;
pub const CLOCK_BOOTTIME: i32 = 7;

/// POSIX timespec structure for high-precision time.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TimeSpec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

/// `sys_clock_gettime` (SYS_CLOCK_GETTIME = 228)
/// Retrieve time of the specified clock.
pub fn sys_clock_gettime(frame: &mut SyscallFrame) -> SyscallResult {
    let clock_id = frame.arg1() as i32;
    let tp_ptr = frame.arg2() as *mut TimeSpec;

    if !is_user_ptr_valid(tp_ptr as u64, core::mem::size_of::<TimeSpec>()) {
        return Err(SyscallError::EFAULT);
    }

    let ts = match clock_id {
        CLOCK_REALTIME | CLOCK_REALTIME_COARSE => {
            let (sec, usec) = crate::drivers::time::cmos_rtc::get_wall_time();
            TimeSpec {
                tv_sec: sec as i64,
                tv_nsec: (usec as i64) * 1000,
            }
        }
        CLOCK_MONOTONIC | CLOCK_MONOTONIC_RAW | CLOCK_MONOTONIC_COARSE | CLOCK_BOOTTIME => {
            let elapsed_ns = crate::arch::timer::hpet::elapsed_ns();
            TimeSpec {
                tv_sec: (elapsed_ns / 1_000_000_000) as i64,
                tv_nsec: (elapsed_ns % 1_000_000_000) as i64,
            }
        }
        CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            let elapsed_ns = crate::arch::timer::hpet::elapsed_ns();
            TimeSpec {
                tv_sec: (elapsed_ns / 1_000_000_000) as i64,
                tv_nsec: (elapsed_ns % 1_000_000_000) as i64,
            }
        }
        _ => return Err(SyscallError::EINVAL),
    };

    // SAFETY: Validated user memory pointer bounds.
    unsafe {
        core::ptr::write_unaligned(tp_ptr, ts);
    }

    Ok(0)
}

/// `sys_nanosleep` (SYS_NANOSLEEP = 35)
/// High-resolution sleep.
pub fn sys_nanosleep(frame: &mut SyscallFrame) -> SyscallResult {
    let req_ptr = frame.arg1() as *const TimeSpec;
    let rem_ptr = frame.arg2() as *mut TimeSpec;

    if !is_user_ptr_valid(req_ptr as u64, core::mem::size_of::<TimeSpec>()) {
        return Err(SyscallError::EFAULT);
    }

    // SAFETY: Validated user memory pointer bounds.
    let req = unsafe { core::ptr::read_unaligned(req_ptr) };
    if req.tv_sec < 0 || req.tv_nsec < 0 || req.tv_nsec >= 1_000_000_000 {
        return Err(SyscallError::EINVAL);
    }

    let target_ns = (req.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(req.tv_nsec as u64);

    let start_ns = crate::arch::timer::hpet::elapsed_ns();
    while crate::arch::timer::hpet::elapsed_ns().saturating_sub(start_ns) < target_ns {
        crate::proc::thread::Thread::yield_cpu();
    }

    if !rem_ptr.is_null() && is_user_ptr_valid(rem_ptr as u64, core::mem::size_of::<TimeSpec>()) {
        // SAFETY: Validated user memory pointer bounds.
        unsafe {
            core::ptr::write_unaligned(rem_ptr, TimeSpec::default());
        }
    }

    Ok(0)
}
