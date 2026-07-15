/// Unix-like time system call implementations.
///
/// Provides the following syscalls:
///
/// | № | Name | Description |
/// |---|------|-------------|
/// | 35  | `nanosleep`    | Sleep for a requested duration |
/// | 96  | `gettimeofday` | Legacy wall-clock query |
/// | 201 | `time`         | Legacy seconds-since-epoch |
/// | 228 | `clock_gettime`| POSIX clock query |
/// | 229 | `clock_getres` | POSIX clock resolution query |
///
/// # Clock sources
///
/// | `clockid_t` | Value | Source |
/// |-------------|-------|--------|
/// | `CLOCK_REALTIME`          | 0 | CMOS RTC (wall time, seconds precision) |
/// | `CLOCK_MONOTONIC`         | 1 | TSC (nanosecond monotonic counter) |
/// | `CLOCK_PROCESS_CPUTIME_ID`| 2 | TSC approximation |
/// | `CLOCK_THREAD_CPUTIME_ID` | 3 | TSC approximation |
/// | `CLOCK_MONOTONIC_RAW`     | 4 | TSC (same as `CLOCK_MONOTONIC`) |
/// | `CLOCK_REALTIME_COARSE`   | 5 | TSC nanoseconds cast to wall-clock shape |
/// | `CLOCK_MONOTONIC_COARSE`  | 6 | TSC (same as `CLOCK_MONOTONIC`) |
/// | `CLOCK_BOOTTIME`          | 7 | TSC (monotonic since boot) |

use crate::drivers::timer::{CmosRtc, Timer, Tsc};
use crate::syscall::{SyscallResult, to_continue, to_continue_unit};
use crate::vm::vma::VmaManager;
use ostd::arch::cpu::context::UserContext;
use ostd::Error;

// ---------------------------------------------------------------------------
// POSIX clock IDs (clockid_t)
// ---------------------------------------------------------------------------

const CLOCK_REALTIME: usize = 0;
const CLOCK_MONOTONIC: usize = 1;
const CLOCK_PROCESS_CPUTIME_ID: usize = 2;
const CLOCK_THREAD_CPUTIME_ID: usize = 3;
const CLOCK_MONOTONIC_RAW: usize = 4;
const CLOCK_REALTIME_COARSE: usize = 5;
const CLOCK_MONOTONIC_COARSE: usize = 6;
const CLOCK_BOOTTIME: usize = 7;

// ---------------------------------------------------------------------------
// Layout-stable kernel ↔ user structs
// ---------------------------------------------------------------------------

/// `struct timespec` as defined by POSIX.
///
/// Matches the x86-64 Linux ABI layout exactly so that it can be written
/// directly into user memory with `copy_to_user`.
struct Timespec {
    /// Whole seconds.
    tv_sec: i64,
    /// Remaining nanoseconds in the range \[0, 999_999_999\].
    tv_nsec: i64,
}

/// `struct timeval` as defined by POSIX.
///
/// Matches the x86-64 Linux ABI layout.
struct Timeval {
    /// Whole seconds.
    tv_sec: i64,
    /// Remaining microseconds in the range \[0, 999_999\].
    tv_usec: i64,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Returns the current wall-clock time in nanoseconds since the Unix epoch.
///
/// Uses the CMOS RTC when available; falls back to the TSC-derived
/// nanosecond counter when the RTC cannot be opened.
fn realtime_ns() -> u64 {
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
fn monotonic_ns() -> u64 {
    Tsc::new().current_time_ns()
}

/// Resolve a `clockid_t` value to nanoseconds.
///
/// Returns `Err(Error::InvalidArgs)` for unknown clock IDs.
fn clock_ns(clockid: usize) -> Result<u64, Error> {
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
fn write_timespec(vm: &VmaManager, user_ptr: usize, ts: Timespec) -> Result<(), Error> {
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
fn write_timeval(vm: &VmaManager, user_ptr: usize, tv: Timeval) -> Result<(), Error> {
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
fn read_timespec(vm: &VmaManager, user_ptr: usize) -> Result<Timespec, Error> {
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
// Syscall handlers
// ---------------------------------------------------------------------------

/// **`clock_gettime(clockid_t clockid, struct timespec *tp)`** — SYS 228
///
/// Reads the current value of the specified clock and stores it in the
/// `timespec` structure pointed to by `tp`.
///
/// # Errors
/// - `EINVAL` if `clockid` is unknown or `tp` is null / unmapped.
pub(crate) fn syscall_clock_gettime(
    arg0: usize, // clockid
    arg1: usize, // *timespec
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    vm: &VmaManager,
    _ctx: &mut UserContext,
) -> SyscallResult {
    to_continue_unit((|| {
        let ns = clock_ns(arg0)?;
        let ts = Timespec {
            tv_sec: (ns / 1_000_000_000) as i64,
            tv_nsec: (ns % 1_000_000_000) as i64,
        };
        write_timespec(vm, arg1, ts)
    })())
}

/// **`clock_getres(clockid_t clockid, struct timespec *res)`** — SYS 229
///
/// Returns the resolution (precision) of the specified clock.
///
/// - TSC-backed clocks: 1 ns resolution → `{0, 1}`.
/// - RTC-backed `CLOCK_REALTIME`: 1 s resolution → `{1, 0}`.
///
/// If `res` is null the call succeeds without writing anything (same
/// behaviour as Linux).
pub(crate) fn syscall_clock_getres(
    arg0: usize, // clockid
    arg1: usize, // *timespec (nullable)
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    vm: &VmaManager,
    _ctx: &mut UserContext,
) -> SyscallResult {
    to_continue_unit((|| {
        // Validate the clock ID first.
        clock_ns(arg0)?;

        // Null `res` is explicitly allowed by POSIX.
        if arg1 == 0 {
            return Ok(());
        }

        let res = match arg0 {
            // RTC has one-second granularity.
            CLOCK_REALTIME | CLOCK_REALTIME_COARSE => Timespec { tv_sec: 1, tv_nsec: 0 },
            // All TSC-backed clocks report 1 ns resolution.
            _ => Timespec { tv_sec: 0, tv_nsec: 1 },
        };
        write_timespec(vm, arg1, res)
    })())
}

/// **`nanosleep(const struct timespec *req, struct timespec *rem)`** — SYS 35
///
/// Suspends the calling thread for at least the duration specified by
/// `req`.  On early wake-up (signal delivery), the remaining time is
/// written to `rem` when `rem` is non-null.
///
/// Because PetraOS does not yet implement a sleep queue, this performs a
/// busy-wait spin loop against the TSC — acceptable for early-stage kernel
/// testing; a blocking implementation can replace the loop later.
///
/// # Errors
/// - `EINVAL` if `req` is null or contains a negative / out-of-range value.
pub(crate) fn syscall_nanosleep(
    arg0: usize, // *req
    arg1: usize, // *rem (nullable)
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    vm: &VmaManager,
    _ctx: &mut UserContext,
) -> SyscallResult {
    to_continue_unit((|| {
        let req = read_timespec(vm, arg0)?;

        if req.tv_sec < 0 || req.tv_nsec < 0 || req.tv_nsec >= 1_000_000_000 {
            return Err(Error::InvalidArgs);
        }

        let sleep_ns = req.tv_sec as u64 * 1_000_000_000 + req.tv_nsec as u64;
        if sleep_ns == 0 {
            return Ok(());
        }

        let start_ns = monotonic_ns();
        let deadline_ns = start_ns.saturating_add(sleep_ns);

        // Busy-wait until the deadline passes.
        // TODO(kernel): replace with a proper timer-interrupt-based sleep queue.
        loop {
            let now = monotonic_ns();
            if now >= deadline_ns {
                break;
            }
            core::hint::spin_loop();
        }

        // Write zero remainder — we always sleep the full duration.
        if arg1 != 0 {
            write_timespec(vm, arg1, Timespec { tv_sec: 0, tv_nsec: 0 })?;
        }

        Ok(())
    })())
}

/// **`gettimeofday(struct timeval *tv, struct timezone *tz)`** — SYS 96
///
/// Legacy wall-clock query.  The timezone argument is ignored (matching
/// modern Linux behaviour).
///
/// # Errors
/// - `EINVAL` if `tv` is null or unmapped.
pub(crate) fn syscall_gettimeofday(
    arg0: usize, // *timeval
    _arg1: usize, // *timezone (ignored)
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    vm: &VmaManager,
    _ctx: &mut UserContext,
) -> SyscallResult {
    to_continue_unit((|| {
        let ns = realtime_ns();
        let tv = Timeval {
            tv_sec: (ns / 1_000_000_000) as i64,
            tv_usec: ((ns % 1_000_000_000) / 1_000) as i64,
        };
        write_timeval(vm, arg0, tv)
    })())
}

/// **`time(time_t *tloc)`** — SYS 201
///
/// Returns the current time as seconds elapsed since the Unix epoch
/// (00:00:00 UTC, 1 January 1970).  If `tloc` is non-null, the value is
/// also stored at the pointed-to location.
///
/// # Errors
/// - `EFAULT` (encoded as `EINVAL`) if `tloc` is non-null but unmapped.
pub(crate) fn syscall_time(
    arg0: usize, // *time_t (nullable)
    _a1: usize,
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    vm: &VmaManager,
    _ctx: &mut UserContext,
) -> SyscallResult {
    let ns = realtime_ns();
    let seconds = (ns / 1_000_000_000) as i64;

    if arg0 != 0 {
        // Write the 8-byte `time_t` value to user space.
        let bytes = seconds.to_ne_bytes();
        if vm.copy_to_user(arg0, &bytes).is_err() {
            return SyscallResult::Continue(-(Error::InvalidArgs as isize) as usize);
        }
    }

    to_continue(Ok(seconds as usize))
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
        assert!(
            t1 >= t0,
            "monotonic clock went backwards: t0={t0} t1={t1}"
        );
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
