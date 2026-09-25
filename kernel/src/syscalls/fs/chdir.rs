//! System calls for process working directory manipulation (`getcwd`, `chdir`, `fchdir`).

use super::*;
use crate::fs::vfs::types::InodeType;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserCStr, UserPtr};

/// `sys_getcwd` (SYS_GETCWD = 79)
/// Get current working directory string.
#[wrap_syscall]
pub fn sys_getcwd(buf: UserPtr<u8>, size: usize) -> SyscallResult {
    if size == 0 || !buf.is_valid_for(size) {
        return Err(SyscallError::EINVAL);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let cwd_bytes = proc.cwd.as_bytes();
    if cwd_bytes.len() + 1 > size {
        return Err(SyscallError::ENOMEM);
    }

    buf.write_slice(cwd_bytes).ok_or(SyscallError::EFAULT)?;
    buf.offset(cwd_bytes.len())
        .write(0)
        .ok_or(SyscallError::EFAULT)?;

    Ok(buf.as_u64() as usize)
}

/// `sys_chdir` (SYS_CHDIR = 80)
/// Change working directory.
#[wrap_syscall]
pub fn sys_chdir(path_ptr: UserCStr) -> SyscallResult {
    let path = path_ptr.to_string(4096)?;

    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let dentry = crate::fs::resolve_path(&full_path)?;
    if dentry.inode.inode_type != InodeType::Directory {
        return Err(SyscallError::ENOTDIR);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    proc.cwd = crate::fs::normalize_path(&proc.cwd, &full_path);

    Ok(0)
}

/// `sys_fchdir` (SYS_FCHDIR = 81)
/// Change working directory using an open directory file descriptor.
#[wrap_syscall]
pub fn sys_fchdir(fd: i32) -> SyscallResult {
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();

    let file = proc.fd_table.get(fd)?;
    if file.dentry.inode.inode_type != InodeType::Directory {
        return Err(SyscallError::ENOTDIR);
    }

    proc.cwd = file.dentry.full_path();
    Ok(0)
}
