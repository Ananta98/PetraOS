use crate::proc::userspace::read_user_string;
use crate::syscall::SyscallResult;
use crate::syscall::to_continue_i32;
use crate::vm::vma::VmaManager;
use ostd::Error;

const F_OK: u32 = 0;
const X_OK: u32 = 1;
const W_OK: u32 = 2;
const R_OK: u32 = 4;

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
    let mode = arg1 as u32;
    let path = match read_user_string(vm, arg0) {
        Ok(p) => p,
        Err(err) => return to_continue_i32(Err(err)),
    };

    let dentry = match crate::fs::vfs::resolve_path(&path) {
        Ok(d) => d,
        Err(err) => return to_continue_i32(Err(err)),
    };

    if mode == F_OK {
        return to_continue_i32(Ok(0));
    }

    let meta = match dentry.inode.metadata() {
        Ok(m) => m,
        Err(err) => return to_continue_i32(Err(err)),
    };

    // Verify mode bits against inode mode permissions
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
        to_continue_i32(Ok(0))
    } else {
        to_continue_i32(Err(Error::InvalidArgs))
    }
}

/// System call entry: check user permissions for a file relative to dirfd (`faccessat(2)`).
pub fn syscall_faccessat(
    _dirfd: usize,
    pathname: usize,
    mode: usize,
    _flags: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    context: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    syscall_access(pathname, mode, 0, 0, 0, 0, vm, context)
}
