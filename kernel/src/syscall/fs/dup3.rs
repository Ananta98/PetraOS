use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::syscall::to_continue_i32;

/// System call entry: duplicate a file descriptor to a specific descriptor number with flags.
pub fn syscall_dup3(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &crate::vm::vma::VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let oldfd = arg0 as i32;
    let newfd = arg1 as i32;
    let flags = arg2 as u32;
    to_continue_i32(Process::current().fd_table.lock().dup3(oldfd, newfd, flags))
}
