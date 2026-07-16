use crate::syscall::{SyscallResult, to_continue};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

use super::realtime_ns;

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
