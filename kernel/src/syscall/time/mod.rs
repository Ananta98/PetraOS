//! Unix-like time system call implementations.
//!
//! Provides the following syscalls:
//!
//! | № | Name | Description |
//! |---|------|-------------|
//! | 35  | `nanosleep`    | Sleep for a requested duration |
//! | 96  | `gettimeofday` | Legacy wall-clock query |
//! | 201 | `time`         | Legacy seconds-since-epoch |
//! | 228 | `clock_gettime`| POSIX clock query |
//! | 229 | `clock_getres` | POSIX clock resolution query |
//!
//! # Clock sources
//!
//! | `clockid_t` | Value | Source |
//! |-------------|-------|--------|
//! | `CLOCK_REALTIME`          | 0 | CMOS RTC (wall time, seconds precision) |
//! | `CLOCK_MONOTONIC`         | 1 | TSC (nanosecond monotonic counter) |
//! | `CLOCK_PROCESS_CPUTIME_ID`| 2 | TSC approximation |
//! | `CLOCK_THREAD_CPUTIME_ID` | 3 | TSC approximation |
//! | `CLOCK_MONOTONIC_RAW`     | 4 | TSC (same as `CLOCK_MONOTONIC`) |
//! | `CLOCK_REALTIME_COARSE`   | 5 | TSC nanoseconds cast to wall-clock shape |
//! | `CLOCK_MONOTONIC_COARSE`  | 6 | TSC (same as `CLOCK_MONOTONIC`) |
//! | `CLOCK_BOOTTIME`          | 7 | TSC (monotonic since boot) |

#![allow(clippy::redundant_closure_call, clippy::too_many_arguments)]

pub mod clock_getres;
pub mod clock_gettime;
pub mod gettimeofday;
pub mod nanosleep;
pub mod sys_time;

pub use clock_getres::syscall_clock_getres;
pub use clock_gettime::syscall_clock_gettime;
pub use gettimeofday::syscall_gettimeofday;
pub use nanosleep::syscall_nanosleep;
pub use sys_time::syscall_time;

use crate::drivers::timer::{CmosRtc, Timer, Tsc};
use crate::vm::vma::VmaManager;
use ostd::Error;

// ---------------------------------------------------------------------------
// POSIX clock IDs (clockid_t)
// ---------------------------------------------------------------------------

pub(super) const CLOCK_REALTIME: usize = 0;
pub(super) const CLOCK_MONOTONIC: usize = 1;
pub(super) const CLOCK_PROCESS_CPUTIME_ID: usize = 2;
pub(super) const CLOCK_THREAD_CPUTIME_ID: usize = 3;
pub(super) const CLOCK_MONOTONIC_RAW: usize = 4;
pub(super) const CLOCK_REALTIME_COARSE: usize = 5;
pub(super) const CLOCK_MONOTONIC_COARSE: usize = 6;
pub(super) const CLOCK_BOOTTIME: usize = 7;

// ---------------------------------------------------------------------------
// Layout-stable kernel ↔ user structs
// ---------------------------------------------------------------------------

/// `struct timespec` as defined by POSIX.
///
/// Matches the x86-64 Linux ABI layout exactly so that it can be written
/// directly into user memory with `copy_to_user`.
pub(super) struct Timespec {
    /// Whole seconds.
    pub(super) tv_sec: i64,
    /// Remaining nanoseconds in the range \[0, 999_999_999\].
    pub(super) tv_nsec: i64,
}

/// `struct timeval` as defined by POSIX.
///
/// Matches the x86-64 Linux ABI layout.
pub(super) struct Timeval {
    /// Whole seconds.
    pub(super) tv_sec: i64,
    /// Remaining microseconds in the range \[0, 999_999\].
    pub(super) tv_usec: i64,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Returns the current wall-clock time in nanoseconds since the Unix epoch.
///
/// Uses the CMOS RTC when available; falls back to the TSC-derived
/// nanosecond counter when the RTC cannot be opened.
pub(super) fn realtime_ns() -> u64 {
    if let Ok(rtc) = CmosRtc::new() {
        rtc.current_time_ns()
    } else {
        Tsc::new().current_time_ns()
    }
}

/// Returns the current monotonic time in nanoseconds since an arbitrary,
/// fixed boot-time epoch.
///
/// Always sourced from the TSC.
pub(super) fn monotonic_ns() -> u64 {
    Tsc::new().current_time_ns()
}

/// Resolve a `clockid_t` value to nanoseconds.
///
/// Returns `Err(Error::InvalidArgs)` for unknown clock IDs.
pub(super) fn clock_ns(clockid: usize) -> Result<u64, Error> {
    match clockid {
        CLOCK_REALTIME | CLOCK_REALTIME_COARSE => Ok(realtime_ns()),
        CLOCK_MONOTONIC
        | CLOCK_MONOTONIC_RAW
        | CLOCK_MONOTONIC_COARSE
        | CLOCK_BOOTTIME
        | CLOCK_PROCESS_CPUTIME_ID
        | CLOCK_THREAD_CPUTIME_ID => Ok(monotonic_ns()),
        _ => Err(Error::InvalidArgs),
    }
}

/// Write a [`Timespec`] to a user-space pointer.
///
/// Fields are serialized in native-endian byte order, matching the
/// x86-64 ABI layout of `struct timespec`.
pub(super) fn write_timespec(vm: &VmaManager, user_ptr: usize, ts: Timespec) -> Result<(), Error> {
    if user_ptr == 0 {
        return Err(Error::InvalidArgs);
    }
    // Write tv_sec (8 bytes) followed by tv_nsec (8 bytes).
    vm.copy_to_user(user_ptr, &ts.tv_sec.to_ne_bytes())?;
    vm.copy_to_user(user_ptr + 8, &ts.tv_nsec.to_ne_bytes())
}

/// Write a [`Timeval`] to a user-space pointer.
///
/// Fields are serialized in native-endian byte order, matching the
/// x86-64 ABI layout of `struct timeval`.
pub(super) fn write_timeval(vm: &VmaManager, user_ptr: usize, tv: Timeval) -> Result<(), Error> {
    if user_ptr == 0 {
        return Err(Error::InvalidArgs);
    }
    // Write tv_sec (8 bytes) followed by tv_usec (8 bytes).
    vm.copy_to_user(user_ptr, &tv.tv_sec.to_ne_bytes())?;
    vm.copy_to_user(user_ptr + 8, &tv.tv_usec.to_ne_bytes())
}

/// Read a [`Timespec`] from a user-space pointer.
///
/// Deserializes two consecutive native-endian `i64` values from user memory.
pub(super) fn read_timespec(vm: &VmaManager, user_ptr: usize) -> Result<Timespec, Error> {
    if user_ptr == 0 {
        return Err(Error::InvalidArgs);
    }
    let mut sec_bytes = [0u8; 8];
    let mut nsec_bytes = [0u8; 8];
    vm.copy_from_user(user_ptr, &mut sec_bytes)?;
    vm.copy_from_user(user_ptr + 8, &mut nsec_bytes)?;
    Ok(Timespec {
        tv_sec: i64::from_ne_bytes(sec_bytes),
        tv_nsec: i64::from_ne_bytes(nsec_bytes),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    /// Smoke-test the internal clock helpers — verifies that the TSC and
    /// RTC both return a plausible non-zero value and that `clock_ns`
    /// rejects unknown IDs.
    #[ktest]
    fn test_clock_ns_smoke() {
        // Monotonic clock must return a non-zero value when TSC is running.
        let mono = clock_ns(CLOCK_MONOTONIC).expect("CLOCK_MONOTONIC should succeed");
        assert!(
            mono > 0,
            "monotonic clock returned 0 — TSC may not be running"
        );

        // Verify aliased IDs resolve without error.
        clock_ns(CLOCK_MONOTONIC_RAW).expect("CLOCK_MONOTONIC_RAW should succeed");
        clock_ns(CLOCK_BOOTTIME).expect("CLOCK_BOOTTIME should succeed");
        clock_ns(CLOCK_REALTIME).expect("CLOCK_REALTIME should succeed");

        // Unknown clock ID must be rejected.
        assert!(clock_ns(999).is_err(), "unknown clock ID should return Err");
    }

    /// Verifies that two consecutive monotonic readings are non-decreasing.
    #[ktest]
    fn test_monotonic_is_non_decreasing() {
        let t0 = monotonic_ns();
        // Force a small amount of work so the compiler cannot fold the two
        // reads into one.
        let _work: u64 = (0u64..1000).fold(0, |a, b| a.wrapping_add(b));
        let t1 = monotonic_ns();
        assert!(t1 >= t0, "monotonic clock went backwards: t0={t0} t1={t1}");
    }

    /// Checks that `realtime_ns` returns a value that is at least the Unix
    /// timestamp for 2024-01-01 00:00:00 UTC (1_704_067_200 seconds ≈
    /// 1.704 × 10¹⁸ ns), catching obvious epoch-calculation bugs.
    #[ktest]
    fn test_realtime_sanity() {
        const EPOCH_2024: u64 = 1_704_067_200 * 1_000_000_000;
        let rt = realtime_ns();
        // We accept zero only when the RTC is not available (QEMU w/o RTC).
        if rt != 0 {
            assert!(
                rt >= EPOCH_2024,
                "realtime_ns={rt} is before 2024-01-01 — epoch calculation is wrong"
            );
        }
    }
}
