use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::Error;

/// System call entry: write to a file descriptor.
pub fn syscall_write(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    SyscallResult::from_result(do_write(arg0 as i32, arg1, arg2, vm))
}

fn do_write(fd: i32, user_buf: usize, len: usize, vm: &VmaManager) -> Result<usize, Error> {
    let mut kbuf = alloc::vec![0u8; len];
    vm.copy_from_user(user_buf, &mut kbuf)
        .map_err(|_| Error::AccessDenied)?;
    Process::current().fd_table.lock().write(fd, &kbuf)
}
