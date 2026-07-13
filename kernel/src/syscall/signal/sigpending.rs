/// `rt_sigpending(set, sigsetsize)` — query the set of pending signals
/// that are blocked for the calling process (SYS_rt_sigpending = 34).
///
/// Writes the set of signals that are currently blocked AND pending for the
/// calling process into the user-space `sigset_t` pointed to by `set`.
///
/// Returns `0` on success, negated `errno` on failure.
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_unit};
use crate::vm::vma::VmaManager;
use ostd::Error;

/// System call entry: `rt_sigpending(set, sigsetsize)`.
pub(crate) fn syscall_rt_sigpending(
    arg0: usize, // sigset_t __user *set
    arg1: usize, // size_t sigsetsize (must == 8)
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let set_ptr = arg0;
    let sigsetsize = arg1;

    if sigsetsize != 8 {
        return to_continue_unit(Err(Error::InvalidArgs));
    }
    if set_ptr == 0 {
        return to_continue_unit(Err(Error::InvalidArgs));
    }

    let process = Process::current();
    let signals = process.signals();

    // POSIX: sigpending returns signals that are both pending AND blocked.
    let pending = signals.queue.pending_snapshot();
    let blocked = signals.queue.get_mask();
    let result = pending.intersection(blocked);

    let raw = result.as_u64().to_le_bytes();
    to_continue_unit(vm.copy_to_user(set_ptr, &raw))
}
