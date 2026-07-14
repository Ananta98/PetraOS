use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::syscall::to_continue_i32;

/// System call entry: duplicate a file descriptor to a specific descriptor number.
pub(crate) fn syscall_dup2(
    arg0: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &crate::vm::vma::VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let oldfd = arg0 as i32;
    let newfd = arg1 as i32;
    to_continue_i32(Process::current().fd_table.lock().dup2(oldfd, newfd))
}
