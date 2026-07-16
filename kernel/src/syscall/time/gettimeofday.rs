use crate::syscall::SyscallResult;
use crate::syscall::to_continue_unit;
use crate::vm::vma::VmaManager;
use ostd::arch::cpu::context::UserContext;

use super::{Timeval, realtime_ns, write_timeval};

/// **`gettimeofday(struct timeval *tv, struct timezone *tz)`** — SYS 96
///
/// Legacy wall-clock query.  The timezone argument is ignored (matching
/// modern Linux behaviour).
///
/// # Errors
/// - `EINVAL` if `tv` is null or unmapped.
pub(crate) fn syscall_gettimeofday(
    arg0: usize,  // *timeval
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
