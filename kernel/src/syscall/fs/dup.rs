use crate::proc::process::Process;
use crate::syscall::SyscallResult;

/// System call entry: duplicate a file descriptor.
pub(crate) fn syscall_dup(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &crate::vm::vma::VmaManager,
) -> SyscallResult {
    super::to_continue_i32(Process::current().fd_table().lock().dup(arg0 as i32))
}
