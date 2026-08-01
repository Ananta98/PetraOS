use crate::proc::userspace::read_user_string;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::Error;

/// System call entry: change owner and group of a file.
pub fn syscall_chown(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    SyscallResult::from_result(do_chown(arg0, arg1 as u32, arg2 as u32, vm))
}

fn do_chown(path_ptr: usize, uid: u32, gid: u32, vm: &VmaManager) -> Result<(), Error> {
    let path = read_user_string(vm, path_ptr)?;
    let dentry = crate::fs::vfs::resolve_path(&path)?;
    dentry.inode.chown(uid, gid)
}
