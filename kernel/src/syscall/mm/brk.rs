use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;

/// Handler for `SYS_brk` (syscall 12).
///
/// `arg0` – new program break address (0 means query only).
///
/// Always returns the (possibly updated) program break.  This matches
/// the Linux `brk` convention: the kernel never returns a negated errno;
/// on failure the old break is returned unchanged.
pub(crate) fn syscall_brk(
    arg0: usize,
    _arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    SyscallResult::Continue(vm.brk(arg0))
}
