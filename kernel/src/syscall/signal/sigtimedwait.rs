use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;

/// System call entry: `rt_sigtimedwait(uthese, uinfo, uts, sigsetsize)`.
pub fn syscall_rt_sigtimedwait(
    _arg0: usize, // const sigset_t __user *uthese
    _arg1: usize, // siginfo_t __user *uinfo
    _arg2: usize, // const struct timespec __user *uts
    _arg3: usize, // size_t sigsetsize
    _arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    _context: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    // TODO(agent): implement rt_sigtimedwait
    SyscallResult::Continue(-38_isize as usize)
}
