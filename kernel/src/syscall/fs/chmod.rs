use crate::syscall::SyscallResult;
use crate::syscall::to_continue_unit;
use crate::vm::vma::VmaManager;

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
    let mode = arg1 as u32;
    match crate::syscall::read_user_string(vm, arg0) {
        Ok(path) => {
            match crate::fs::vfs::resolve_path(&path) {
                Ok(dentry) => to_continue_unit(dentry.inode.chmod(mode)),
                Err(error) => to_continue_unit(Err(error)),
            }
        },
        Err(error) => to_continue_unit(Err(error)),
    }
}
