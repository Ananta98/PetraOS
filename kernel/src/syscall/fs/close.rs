use crate::proc::process::Process;
use crate::syscall::SyscallResult;

/// System call entry: close a file descriptor.
pub(crate) fn syscall_close(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &crate::vm::vma::VmaManager,
) -> SyscallResult {
    super::to_continue_unit(Process::current().fd_table().lock().close(arg0 as i32))
}
