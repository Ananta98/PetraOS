use crate::syscall::SyscallResult;
use crate::syscall::to_continue_unit;
use crate::vm::vma::VmaManager;
use ostd::arch::cpu::context::UserContext;

use super::{CLOCK_REALTIME, CLOCK_REALTIME_COARSE, Timespec, clock_ns, write_timespec};

/// **`clock_getres(clockid_t clockid, struct timespec *res)`** — SYS 229
///
/// Returns the resolution (precision) of the specified clock.
///
/// - TSC-backed clocks: 1 ns resolution → `{0, 1}`.
/// - RTC-backed `CLOCK_REALTIME`: 1 s resolution → `{1, 0}`.
///
/// If `res` is null the call succeeds without writing anything (same
/// behaviour as Linux).
pub fn syscall_clock_getres(
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
            CLOCK_REALTIME | CLOCK_REALTIME_COARSE => Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            },
            // All TSC-backed clocks report 1 ns resolution.
            _ => Timespec {
                tv_sec: 0,
                tv_nsec: 1,
            },
        };
        write_timespec(vm, arg1, res)
    })())
}
