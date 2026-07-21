use crate::syscall::SyscallResult;
use crate::syscall::to_continue_unit;
use crate::vm::vma::VmaManager;

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
    let uid = arg1 as u32;
    let gid = arg2 as u32;
    match crate::syscall::read_user_string(vm, arg0) {
        Ok(path) => {
            match crate::fs::vfs::resolve_path(&path) {
                Ok(dentry) => to_continue_unit(dentry.inode.chown(uid, gid)),
                Err(error) => to_continue_unit(Err(error)),
            }
        },
        Err(error) => to_continue_unit(Err(error)),
    }
}
