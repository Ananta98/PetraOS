use crate::proc::userspace::read_user_string;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::Error;

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
    SyscallResult::from_result(do_chdir(arg0, vm))
}

fn do_chdir(path_ptr: usize, vm: &VmaManager) -> Result<(), Error> {
    let path = read_user_string(vm, path_ptr)?;
    let dentry = crate::fs::vfs::resolve_path(&path)?;
    let metadata = dentry.inode.metadata()?;
    if metadata.file_type != crate::fs::vfs::FileType::Directory {
        return Err(Error::InvalidArgs);
    }
    *crate::fs::vfs::CWD_DENTRY.lock() = Some(dentry);
    Ok(())
}
