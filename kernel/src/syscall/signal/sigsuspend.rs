use crate::ipc::SigSet;
/// `rt_sigsuspend(mask, sigsetsize)` — atomically replace signal mask and
/// suspend until a signal is delivered (SYS_rt_sigsuspend = 72).
///
/// POSIX semantics:
/// 1. Save the current signal mask.
/// 2. Atomically replace it with `mask` (allowing the caller to unblock
///    certain signals).
/// 3. Suspend execution (sleep) until a signal whose action is to terminate
///    the process or invoke a signal handler is delivered.
/// 4. Restore the previous signal mask before returning.
/// 5. Always returns `-EINTR` (the sleep was interrupted by a signal).
///
/// # Current limitations
///
/// Full sleep requires a `WaitQueue` tied to the per-process signal state.
/// This implementation performs a busy-wait spin using
/// `core::hint::spin_loop()`, which is functionally correct but wastes CPU
/// time. A future patch will add a proper `WaitQueue` to `ProcessSignals`
/// and park the calling task until `SigQueue::has_pending()` returns `true`.
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_unit};
use crate::vm::vma::VmaManager;
use ostd::Error;

/// System call entry: `rt_sigsuspend(mask, sigsetsize)`.
pub fn syscall_rt_sigsuspend(
    arg0: usize, // const sigset_t __user *mask
    arg1: usize, // size_t sigsetsize (must == 8)
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let mask_ptr = arg0;
    let sigsetsize = arg1;

    if sigsetsize != 8 {
        return to_continue_unit(Err(Error::InvalidArgs));
    }
    if mask_ptr == 0 {
        return to_continue_unit(Err(Error::InvalidArgs));
    }

    // Read the temporary mask from user space.
    let mut raw = [0u8; 8];
    if let Err(err) = vm.copy_from_user(mask_ptr, &mut raw) {
        return to_continue_unit(Err(err));
    }
    let temp_mask = SigSet::from_u64(u64::from_le_bytes(raw));

    let process = Process::current();
    let signals = process.signals.clone();

    // ── Atomically save old mask and install temp mask ────────────────────
    let saved_mask = signals.queue.get_mask();
    signals.queue.set_mask(temp_mask);

    // ── Suspend until a signal becomes deliverable ────────────────────────
    //
    // Spin until the queue reports a pending unblocked signal.
    // In a future revision: park the task on a WaitQueue and wake it from
    // SigQueue::enqueue() when a new signal arrives.
    loop {
        if signals.queue.has_pending() {
            break;
        }
        core::hint::spin_loop();
    }

    // ── Restore the saved mask ────────────────────────────────────────────
    signals.queue.set_mask(saved_mask);

    // sigsuspend always returns -EINTR (interrupted by signal).
    // ostd::Error does not have an Interrupted variant; IoError maps to EINTR
    // for signal-interrupted system calls.
    to_continue_unit(Err(Error::IoError))
}
