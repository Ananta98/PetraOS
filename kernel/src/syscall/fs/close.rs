use crate::proc::process::Process;
use crate::syscall::SyscallResult;

/// System call entry: close a file descriptor.
pub fn syscall_close(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &crate::vm::vma::VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    SyscallResult::from_result(Process::current().fd_table.lock().close(arg0 as i32))
}
