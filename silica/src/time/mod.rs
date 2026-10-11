// SPDX-License-Identifier: GPL-2.0

//! Time and Clock Management Subsystem.
//!
//! Provides the Unix Clocksource framework, high-resolution TSC timer,
//! CMOS RTC driver, and unified POSIX clock domain timekeeping.

pub mod clocksource;
pub mod rtc;
pub mod tsc;

use core::sync::atomic::{AtomicI64, AtomicU64, Ordering};

pub use clocksource::{
    Clocksource, ClocksourceFlags, active_clocksource_name, is_high_resolution,
    monotonic_now_ns, register_clocksource,
};
pub use rtc::{
    CmosRtc, RtcDateTime, datetime_to_unix_epoch, get_cmos_epoch, get_cmos_time,
};
pub use tsc::{JiffiesClocksource, TscClocksource};

use crate::api::errno::{Errno, Result};

// ----------------------------------------------------------------------------
// POSIX Clock Identifiers (Linux compatible)
// ----------------------------------------------------------------------------

pub const CLOCK_REALTIME: i32 = 0;
pub const CLOCK_MONOTONIC: i32 = 1;
pub const CLOCK_PROCESS_CPUTIME_ID: i32 = 2;
pub const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
pub const CLOCK_MONOTONIC_RAW: i32 = 4;
pub const CLOCK_REALTIME_COARSE: i32 = 5;
pub const CLOCK_MONOTONIC_COARSE: i32 = 6;
pub const CLOCK_BOOTTIME: i32 = 7;
pub const CLOCK_REALTIME_ALARM: i32 = 8;
pub const CLOCK_BOOTTIME_ALARM: i32 = 9;
pub const CLOCK_TAI: i32 = 11;

// ----------------------------------------------------------------------------
// Timespec & Timeval Data Structures
// ----------------------------------------------------------------------------

/// POSIX / Linux x86_64 compatible `struct timespec` (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

impl Timespec {
    pub const ZERO: Self = Self {
        tv_sec: 0,
        tv_nsec: 0,
    };

    pub fn new(sec: i64, nsec: i64) -> Self {
        Self {
            tv_sec: sec,
            tv_nsec: nsec,
        }
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0..8].copy_from_slice(&self.tv_sec.to_ne_bytes());
        buf[8..16].copy_from_slice(&self.tv_nsec.to_ne_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 16 {
            return None;
        }
        let tv_sec = i64::from_ne_bytes(buf[0..8].try_into().ok()?);
        let tv_nsec = i64::from_ne_bytes(buf[8..16].try_into().ok()?);
        Some(Self { tv_sec, tv_nsec })
    }

    pub fn is_valid(&self) -> bool {
        self.tv_sec >= 0 && self.tv_nsec >= 0 && self.tv_nsec < 1_000_000_000
    }

    pub fn total_nanos(&self) -> Option<u64> {
        if !self.is_valid() {
            return None;
        }
        (self.tv_sec as u64)
            .checked_mul(1_000_000_000)?
            .checked_add(self.tv_nsec as u64)
    }
}

/// POSIX / Linux x86_64 compatible `struct timeval` (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timeval {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

impl Timeval {
    pub fn new(sec: i64, usec: i64) -> Self {
        Self {
            tv_sec: sec,
            tv_usec: usec,
        }
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0..8].copy_from_slice(&self.tv_sec.to_ne_bytes());
        buf[8..16].copy_from_slice(&self.tv_usec.to_ne_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 16 {
            return None;
        }
        let tv_sec = i64::from_ne_bytes(buf[0..8].try_into().ok()?);
        let tv_usec = i64::from_ne_bytes(buf[8..16].try_into().ok()?);
        Some(Self { tv_sec, tv_usec })
    }
}

// ----------------------------------------------------------------------------
// Wall-Clock Epoch & Offset State
// ----------------------------------------------------------------------------

/// System wall-clock UNIX epoch timestamp at boot time (in seconds).
static BOOT_EPOCH_SECS: AtomicU64 = AtomicU64::new(1775563440);

/// Realtime wall-clock offset in nanoseconds (for adjtime / clock_settime).
static REALTIME_OFFSET_NS: AtomicI64 = AtomicI64::new(0);

// ----------------------------------------------------------------------------
// Time Query APIs
// ----------------------------------------------------------------------------

/// Returns the wall-clock epoch timestamp at boot in seconds since UNIX epoch.
pub fn get_boot_epoch_secs() -> u64 {
    BOOT_EPOCH_SECS.load(Ordering::Relaxed)
}

/// Sets the boot epoch timestamp in seconds.
pub fn set_boot_epoch_secs(epoch: u64) {
    BOOT_EPOCH_SECS.store(epoch, Ordering::Relaxed);
}

/// Adjusts the real-time wall clock offset in nanoseconds.
pub fn adjust_realtime_offset(delta_ns: i64) {
    REALTIME_OFFSET_NS.fetch_add(delta_ns, Ordering::Relaxed);
}

/// Returns the current monotonic time in nanoseconds since boot.
pub fn get_monotonic_ns() -> u64 {
    clocksource::monotonic_now_ns()
}

/// Returns the current real-time wall-clock time in nanoseconds since UNIX epoch.
pub fn get_realtime_ns() -> u64 {
    let boot_ns = (BOOT_EPOCH_SECS.load(Ordering::Relaxed) as u128) * 1_000_000_000;
    let mono_ns = get_monotonic_ns() as u128;
    let offset = REALTIME_OFFSET_NS.load(Ordering::Relaxed) as i128;

    let total = (boot_ns + mono_ns).saturating_add_signed(offset);
    total as u64
}

/// Returns the current time of day in microsecond precision.
pub fn get_timeval() -> Timeval {
    let ns = get_realtime_ns();
    Timeval {
        tv_sec: (ns / 1_000_000_000) as i64,
        tv_usec: ((ns % 1_000_000_000) / 1000) as i64,
    }
}

/// Returns the current time of the requested clock domain in nanosecond precision.
pub fn get_timespec(clock_id: i32) -> Result<Timespec> {
    match clock_id {
        CLOCK_REALTIME | CLOCK_REALTIME_ALARM | CLOCK_TAI => {
            let ns = get_realtime_ns();
            Ok(Timespec {
                tv_sec: (ns / 1_000_000_000) as i64,
                tv_nsec: (ns % 1_000_000_000) as i64,
            })
        }
        CLOCK_REALTIME_COARSE => {
            let elapsed = ostd::timer::Jiffies::elapsed().as_duration();
            let boot_epoch = get_boot_epoch_secs();
            Ok(Timespec {
                tv_sec: (boot_epoch + elapsed.as_secs()) as i64,
                tv_nsec: elapsed.subsec_nanos() as i64,
            })
        }
        CLOCK_MONOTONIC | CLOCK_MONOTONIC_RAW | CLOCK_BOOTTIME | CLOCK_BOOTTIME_ALARM => {
            let ns = get_monotonic_ns();
            Ok(Timespec {
                tv_sec: (ns / 1_000_000_000) as i64,
                tv_nsec: (ns % 1_000_000_000) as i64,
            })
        }
        CLOCK_MONOTONIC_COARSE => {
            let elapsed = ostd::timer::Jiffies::elapsed().as_duration();
            Ok(Timespec {
                tv_sec: elapsed.as_secs() as i64,
                tv_nsec: elapsed.subsec_nanos() as i64,
            })
        }
        _ => Err(Errno::EINVAL),
    }
}

/// Returns the resolution of the requested clock domain.
pub fn get_resolution(clock_id: i32) -> Result<Timespec> {
    match clock_id {
        CLOCK_REALTIME | CLOCK_REALTIME_ALARM | CLOCK_TAI
        | CLOCK_MONOTONIC | CLOCK_MONOTONIC_RAW | CLOCK_BOOTTIME | CLOCK_BOOTTIME_ALARM
        | CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            if is_high_resolution() {
                Ok(Timespec {
                    tv_sec: 0,
                    tv_nsec: 1, // 1 nanosecond resolution with TSC
                })
            } else {
                Ok(Timespec {
                    tv_sec: 0,
                    tv_nsec: 1_000_000, // 1 millisecond with Jiffies
                })
            }
        }
        CLOCK_REALTIME_COARSE | CLOCK_MONOTONIC_COARSE => Ok(Timespec {
            tv_sec: 0,
            tv_nsec: 1_000_000, // 1 ms resolution (1000 Hz)
        }),
        _ => Err(Errno::EINVAL),
    }
}

/// Returns the number of clock ticks elapsed since boot in the requested frequency (Hz).
pub fn get_uptime_ticks(hz: u64) -> u64 {
    let mono_ns = get_monotonic_ns() as u128;
    ((mono_ns * hz as u128) / 1_000_000_000) as u64
}

// ----------------------------------------------------------------------------
// Subsystem Initialization
// ----------------------------------------------------------------------------

/// Initializes the timekeeping subsystem, bringing up RTC, clocksources, and timer hooks.
pub fn init() -> Result<()> {
    ostd::info!("Time: Initializing timekeeping subsystem...");

    // 1. Initialize CMOS RTC driver and read boot epoch
    if let Err(err) = rtc::init() {
        ostd::warn!("Time: RTC bring-up failed: {:?}", err);
    } else if let Ok(epoch) = rtc::get_cmos_epoch() {
        if epoch > 0 {
            BOOT_EPOCH_SECS.store(epoch, Ordering::Relaxed);
            ostd::info!("Time: Boot wall-clock epoch set from RTC to {}s", epoch);
        }
    }

    // 2. Initialize TSC and Jiffies clocksources
    tsc::init()?;

    // 3. Register periodic cycle accumulator on timer tick
    ostd::timer::register_callback_on_cpu(clocksource::accumulate_cycles);

    ostd::info!(
        "Time: Active clocksource '{}' (high-res: {})",
        active_clocksource_name(),
        is_high_resolution()
    );

    Ok(())
}
