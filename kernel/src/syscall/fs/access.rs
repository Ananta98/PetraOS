use crate::proc::userspace::read_user_string;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::Error;

const F_OK: u32 = 0;
const X_OK: u32 = 1;
const W_OK: u32 = 2;
const R_OK: u32 = 4;

pub const ENOENT: isize = 2;

/// System call entry: check user permissions for a file (`access(2)`).
pub fn syscall_access(
    arg0: usize,
    arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    match do_access(arg0, arg1 as u32, vm) {
        Ok(res) => SyscallResult::Return(res as usize),
        Err(Error::InvalidArgs) => SyscallResult::Return((-ENOENT) as usize),
        Err(e) => SyscallResult::from_err(e),
    }
}

/// System call entry: check user permissions relative to directory fd (`faccessat(2)`).
pub fn syscall_faccessat(
    _dirfd: usize,
    pathname: usize,
    mode: usize,
    _flags: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    match do_access(pathname, mode as u32, vm) {
        Ok(res) => SyscallResult::Return(res as usize),
        Err(Error::InvalidArgs) => SyscallResult::Return((-ENOENT) as usize),
        Err(e) => SyscallResult::from_err(e),
    }
}

fn do_access(path_ptr: usize, mode: u32, vm: &VmaManager) -> Result<i32, Error> {
    let path = read_user_string(vm, path_ptr)?;
    let dentry = crate::fs::vfs::resolve_path(&path)?;

    if mode == F_OK {
        return Ok(0);
    }

    let meta = dentry.inode.metadata()?;
    let user_perm = (meta.mode >> 6) & 0o7;
    let mut ok = true;
    if (mode & R_OK) != 0 && (user_perm & 0o4) == 0 {
        ok = false;
    }
    if (mode & W_OK) != 0 && (user_perm & 0o2) == 0 {
        ok = false;
    }
    if (mode & X_OK) != 0 && (user_perm & 0o1) == 0 {
        ok = false;
    }

    if ok {
        Ok(0)
    } else {
        Err(Error::InvalidArgs)
    }
}
