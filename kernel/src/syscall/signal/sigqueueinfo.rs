use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;

/// System call entry: `rt_sigqueueinfo(pid, sig, uinfo)`.
pub fn syscall_rt_sigqueueinfo(
    _arg0: usize, // pid_t pid
    _arg1: usize, // int sig
    _arg2: usize, // siginfo_t __user *uinfo
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    _context: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    // TODO(agent): implement rt_sigqueueinfo
    SyscallResult::Continue(-38_isize as usize)
}

/// System call entry: `rt_tgsigqueueinfo(tgid, tid, sig, uinfo)`.
pub fn syscall_rt_tgsigqueueinfo(
    _arg0: usize, // pid_t tgid
    _arg1: usize, // pid_t tid
    _arg2: usize, // int sig
    _arg3: usize, // siginfo_t __user *uinfo
    _arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    _context: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    // TODO(agent): implement rt_tgsigqueueinfo
    SyscallResult::Continue(-38_isize as usize)
}
