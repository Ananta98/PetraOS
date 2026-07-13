use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::syscall::to_continue_i32;

/// System call entry: duplicate a file descriptor.
pub(crate) fn syscall_dup(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &crate::vm::vma::VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    to_continue_i32(Process::current().fd_table().lock().dup(arg0 as i32))
}
