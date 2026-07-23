use crate::proc::userspace::{read_user_slice, read_user_string};
use crate::syscall::SyscallResult;
use crate::syscall::to_continue_unit;
use crate::vm::vma::VmaManager;

/// System call entry: change working directory.
pub fn syscall_chdir(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    match read_user_string(vm, arg0) {
        Ok(path) => {
            match crate::fs::vfs::resolve_path(&path) {
                Ok(dentry) => {
                    if let Ok(metadata) = dentry.inode.metadata() {
                        if metadata.file_type != crate::fs::vfs::FileType::Directory {
                            // ENOTDIR would be mapped to InvalidArgs or similar
                            return to_continue_unit(Err(ostd::Error::InvalidArgs));
                        }
                    }
                    *crate::fs::vfs::CWD_DENTRY.lock() = Some(dentry);
                    to_continue_unit(Ok(()))
                }
                Err(error) => to_continue_unit(Err(error)),
            }
        }
        Err(error) => to_continue_unit(Err(error)),
    }
}
