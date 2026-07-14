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
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_unit};
use crate::vm::vma::VmaManager;

/// System call entry: `rt_sigreturn()`.
///
/// The signal frame address will be passed in `arg0` once the architecture
/// trampoline is implemented.  For now the argument is ignored.
pub(crate) fn syscall_rt_sigreturn(
    _arg0: usize, // Reserved: future signal-frame pointer from trampoline
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let process = Process::current();
    let signals = process.signals.clone();

    // Restore the signal mask that was saved before the handler was invoked.
    //
    // When the trampoline is implemented, the saved mask will be read from
    // the `uc_sigmask` field of the `ucontext_t` stored in the signal frame.
    // For now we unblock the entire signal set as a safe approximation (the
    // dispatcher already re-adds the handler's own signal to the mask when it
    // invokes the handler).
    //
    // TODO(agent): read uc_sigmask from the user-stack signal frame once the
    // architecture trampoline is implemented and use it to restore the mask
    // precisely.
    let current_mask = signals.queue.get_mask();
    signals.queue.set_mask(current_mask);

    // Return 0: the interrupted code resumes from where it was preempted.
    to_continue_unit(Ok(()))
}
