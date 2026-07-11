use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::Error;

/// System call entry: write to a file descriptor.
pub(crate) fn syscall_write(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
) -> SyscallResult {
    let fd = arg0 as i32;
    let user_buf = arg1;
    let len = arg2;
    let mut kbuf = alloc::vec![0u8; len];
    if vm.copy_from_user(user_buf, &mut kbuf).is_err() {
        return super::to_continue(Err(Error::AccessDenied));
    }
    super::to_continue(Process::current().fd_table().lock().write(fd, &kbuf))
}
