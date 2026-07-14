use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::syscall::to_continue;
use crate::vm::vma::VmaManager;
use ostd::Error;

/// System call entry: read from a file descriptor.
pub(crate) fn syscall_read(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let user_buf = arg1;
    let len = arg2;
    let mut kbuf = alloc::vec![0u8; len];
    let bytes = match Process::current().fd_table.lock().read(fd, &mut kbuf) {
        Ok(bytes) => bytes,
        Err(error) => return to_continue(Err(error)),
    };
    if vm.copy_to_user(user_buf, &kbuf[..bytes]).is_err() {
        return to_continue(Err(Error::AccessDenied));
    }
    to_continue(Ok(bytes))
}
