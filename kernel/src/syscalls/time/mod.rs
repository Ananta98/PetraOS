use super::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;

// ── Modular syscall submodules ──────────────────────────────────────────
pub mod gettimeofday;
pub mod times;
pub mod clock_gettime;
pub mod nanosleep;

pub use gettimeofday::sys_gettimeofday;
pub use times::sys_times;
pub use clock_gettime::sys_clock_gettime;
pub use nanosleep::sys_nanosleep;


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
