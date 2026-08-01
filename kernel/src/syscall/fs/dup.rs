use crate::proc::process::Process;
use crate::syscall::SyscallResult;

/// System call entry: duplicate a file descriptor.
pub fn syscall_dup(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &crate::vm::vma::VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    SyscallResult::from_result(Process::current().fd_table.lock().dup(arg0 as i32))
}
