use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;

pub fn syscall_sched_get_priority_max(
    _arg0: usize,
    _arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    _context: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    SyscallResult::Continue(-38_isize as usize)
}

pub fn syscall_sched_get_priority_min(
    _arg0: usize,
    _arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    _context: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    SyscallResult::Continue(-38_isize as usize)
}
