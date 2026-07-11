use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;

/// System call entry: terminate the calling process.
pub(crate) fn syscall_exit(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
) -> SyscallResult {
    SyscallResult::Exit(arg0 as i32)
}
