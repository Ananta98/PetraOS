/// `rt_sigreturn()` — return from a user-space signal handler
/// (SYS_rt_sigreturn = 15).
///
/// When the kernel delivers a signal to a user-space handler it:
/// 1. Saves the interrupted CPU register state (the "signal frame") on the
///    user stack.
/// 2. Pushes a return address that points to a signal trampoline which calls
///    `rt_sigreturn`.
/// 3. Jumps to the user's handler.
///
/// When the handler returns it falls through to the trampoline, which invokes
/// this syscall.  The kernel must then:
/// 1. Restore the saved register state from the signal frame on the user stack.
/// 2. Restore the signal mask that was active before the signal was delivered.
/// 3. Resume the interrupted code.
///
/// # Current implementation status
///
/// Full signal frame save/restore requires architecture-specific code in the
/// user-mode entry/exit path (`proc::user`) to lay out and read back the
/// `ucontext_t` structure on the user stack.  That trampoline layer will be
/// added in a future patch.
///
/// For now `rt_sigreturn` is registered so that user-space programs that
/// call it do not receive `-EINVAL`, and the signal mask restoration is
/// performed.  CPU register restoration is a no-op until the trampoline
/// provides the saved `ucontext_t` address (passed via `arg0` when the
/// trampoline layer is wired up).
///
/// Returns: does not normally return — the interrupted context is resumed.
/// If restoration is not possible, returns `0` (continue normally).
use crate::ipc::SigSet;
use crate::proc::process::Process;
use crate::syscall::{SyscallResult};
use crate::vm::vma::VmaManager;

/// System call entry: `rt_sigreturn()`.
///
/// Restores the saved CPU register context and precise signal mask (`uc_sigmask`)
/// from the `SignalFrame` on the user stack.
pub fn syscall_rt_sigreturn(
    arg0: usize, // Signal-frame pointer from trampoline (or 0 to use context.rsp())
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    context: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let process = Process::current();
    let signals = process.signals.clone();

    let sp = if arg0 != 0 { arg0 } else { context.rsp() };

    match crate::arch::signal::restore_signal_frame(vm, context, sp) {
        Ok(sigmask) => {
            signals.queue.set_mask(SigSet::from_u64(sigmask));
            SyscallResult::from_result(Ok(()))
        }
        Err(err) => SyscallResult::from_err(err),
    }
}
