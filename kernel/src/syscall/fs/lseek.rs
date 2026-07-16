use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::syscall::to_continue;

/// System call entry: adjust the file offset.
pub(crate) fn syscall_lseek(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &crate::vm::vma::VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let offset = arg1 as isize;
    let whence = arg2 as i32;
    to_continue(Process::current().fd_table.lock().lseek(fd, offset, whence))
}
