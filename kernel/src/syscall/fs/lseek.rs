use crate::proc::process::Process;
use crate::syscall::SyscallResult;

/// System call entry: adjust the file offset.
pub(crate) fn syscall_lseek(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &crate::vm::vma::VmaManager,
) -> SyscallResult {
    let fd = arg0 as i32;
    let offset = arg1 as isize;
    let whence = arg2 as i32;
    super::to_continue(
        Process::current()
            .fd_table()
            .lock()
            .lseek(fd, offset, whence),
    )
}
