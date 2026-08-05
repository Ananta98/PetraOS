use crate::proc::process::Process;
use crate::proc::userspace::read_user_string;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::Error;

pub const ENOENT: isize = 2;

/// System call entry: open a file.
pub fn syscall_open(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let flags = arg1 as u32;
    let mode = arg2 as u32;
    let path = match read_user_string(vm, arg0) {
        Ok(p) => p,
        Err(e) => return SyscallResult::from_err(e),
    };

    match Process::current().fd_table.lock().open(&path, flags, mode) {
        Ok(fd) => SyscallResult::Return(fd as usize),
        Err(Error::InvalidArgs) => {
            // Path not found / resolution failed -> POSIX -ENOENT (-2)
            SyscallResult::Return((-ENOENT) as usize)
        }
        Err(e) => SyscallResult::from_err(e),
    }
}
