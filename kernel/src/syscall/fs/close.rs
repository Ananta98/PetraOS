use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::syscall::to_continue_unit;

/// System call entry: close a file descriptor.
pub(crate) fn syscall_close(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &crate::vm::vma::VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    to_continue_unit(Process::current().fd_table.lock().close(arg0 as i32))
}
