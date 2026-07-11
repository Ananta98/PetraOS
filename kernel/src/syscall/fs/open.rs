use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;

/// System call entry: open a file.
pub(crate) fn syscall_open(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
) -> SyscallResult {
    let flags = arg1 as u32;
    let mode = arg2 as u32;
    match super::read_user_string(vm, arg0) {
        Ok(path) => super::to_continue_i32(
            Process::current()
                .fd_table()
                .lock()
                .open(&path, flags, mode),
        ),
        Err(error) => super::to_continue_i32(Err(error)),
    }
}
