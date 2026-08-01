use crate::proc::userspace::read_user_string;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::Error;

/// System call entry: change permissions of a file.
pub fn syscall_chmod(
    arg0: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    SyscallResult::from_result(do_chmod(arg0, arg1 as u32, vm))
}

fn do_chmod(path_ptr: usize, mode: u32, vm: &VmaManager) -> Result<(), Error> {
    let path = read_user_string(vm, path_ptr)?;
    let dentry = crate::fs::vfs::resolve_path(&path)?;
    dentry.inode.chmod(mode)
}
