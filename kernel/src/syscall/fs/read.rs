use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::Error;

/// System call entry: read from a file descriptor.
pub fn syscall_read(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    SyscallResult::from_result(do_read(arg0 as i32, arg1, arg2, vm))
}

fn do_read(fd: i32, user_buf: usize, len: usize, vm: &VmaManager) -> Result<usize, Error> {
    if len == 0 {
        // According to Linux man 2 read: if count is zero, read() may detect errors
        // (e.g., bad fd). If no error is detected, read() returns 0.
        let mut dummy = [];
        let _ = Process::current().fd_table.lock().read(fd, &mut dummy)?;
        return Ok(0);
    }

    const MAX_READ_SIZE: usize = 1024 * 1024;
    let capped_len = len.min(MAX_READ_SIZE);
    let mut kbuf = alloc::vec![0u8; capped_len];
    let bytes = Process::current().fd_table.lock().read(fd, &mut kbuf)?;
    vm.copy_to_user(user_buf, &kbuf[..bytes])
        .map_err(|_| Error::AccessDenied)?;
    Ok(bytes)
}
