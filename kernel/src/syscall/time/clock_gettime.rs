use crate::syscall::SyscallResult;
use crate::syscall::to_continue_unit;
use crate::vm::vma::VmaManager;
use ostd::arch::cpu::context::UserContext;

use super::{Timespec, clock_ns, write_timespec};

/// **`clock_gettime(clockid_t clockid, struct timespec *tp)`** — SYS 228
///
/// Reads the current value of the specified clock and stores it in the
/// `timespec` structure pointed to by `tp`.
///
/// # Errors
/// - `EINVAL` if `clockid` is unknown or `tp` is null / unmapped.
pub fn syscall_clock_gettime(
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
