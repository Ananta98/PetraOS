/// `rt_sigprocmask(how, set, oldset, sigsetsize)` — examine and change the
/// calling process's signal mask (SYS_rt_sigprocmask = 14).
///
/// `how` values (matching Linux ABI):
/// - `SIG_BLOCK   = 0` — add signals in `set` to the current mask.
/// - `SIG_UNBLOCK = 1` — remove signals in `set` from the current mask.
/// - `SIG_SETMASK = 2` — replace the current mask with `set`.
///
/// `set` may be NULL (only the `oldset` query is performed).
/// `oldset` may be NULL (the current mask is not returned).
///
/// Returns `0` on success, negated `errno` on failure.
use crate::ipc::SigSet;
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_unit};
use crate::vm::vma::VmaManager;
use ostd::Error;

/// `SIG_BLOCK` — OR the given mask into the current blocked set.
const SIG_BLOCK: usize = 0;
/// `SIG_UNBLOCK` — AND NOT the given mask into the current blocked set.
const SIG_UNBLOCK: usize = 1;
/// `SIG_SETMASK` — replace the blocked set entirely.
const SIG_SETMASK: usize = 2;

/// System call entry: `rt_sigprocmask(how, set, oldset, sigsetsize)`.
pub(crate) fn syscall_rt_sigprocmask(
    arg0: usize, // int how
    arg1: usize, // const sigset_t __user *set   (0 = NULL)
    arg2: usize, // sigset_t __user *oldset       (0 = NULL)
    arg3: usize, // size_t sigsetsize (must == 8)
    _: usize,
    _: usize,
    vm: &VmaManager,
) -> SyscallResult {
    let how = arg0;
    let set_ptr = arg1;
    let oldset_ptr = arg2;
    let sigsetsize = arg3;

    if sigsetsize != 8 {
        return to_continue_unit(Err(Error::InvalidArgs));
    }

    let process = Process::current();
    let signals = process.signals();

    // ── Write current mask to user before modifying ───────────────────────
    if oldset_ptr != 0 {
        let current_mask = signals.queue.get_mask();
        let raw = current_mask.as_u64().to_le_bytes();
        if let Err(err) = vm.copy_to_user(oldset_ptr, &raw) {
            return to_continue_unit(Err(err));
        }
    }

    // ── Apply the new mask if provided ────────────────────────────────────
    if set_ptr != 0 {
        let mut raw = [0u8; 8];
        if let Err(err) = vm.copy_from_user(set_ptr, &mut raw) {
            return to_continue_unit(Err(err));
        }
        let new_set = SigSet::from_u64(u64::from_le_bytes(raw));

        match how {
            SIG_BLOCK => signals.queue.block(new_set),
            SIG_UNBLOCK => signals.queue.unblock(new_set),
            SIG_SETMASK => signals.queue.set_mask(new_set),
            _ => return to_continue_unit(Err(Error::InvalidArgs)),
        }
    }

    to_continue_unit(Ok(()))
}
