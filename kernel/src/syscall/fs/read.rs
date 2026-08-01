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
    let mut kbuf = alloc::vec![0u8; len];
    loop {
        let bytes = Process::current().fd_table.lock().read(fd, &mut kbuf)?;
        if bytes > 0 || fd != 0 {
            vm.copy_to_user(user_buf, &kbuf[..bytes])
                .map_err(|_| Error::AccessDenied)?;
            return Ok(bytes);
        }
        ostd::task::Task::yield_now();
    }
}
