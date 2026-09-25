//! System calls for opening and creating files (`open`, `openat`).

use super::*;
use crate::fs::File;
use crate::fs::vfs::perm::{check_access_stat, R_OK, W_OK, X_OK};
use crate::fs::vfs::types::InodeType;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserCStr};
use alloc::sync::Arc;

/// Parent directory of an absolute path (`/a/b` -> `/a`, `/x` -> `/`).
fn parent_dir_of(abs_path: &str) -> &str {
    match abs_path.rfind('/') {
        Some(0) | None => "/",
        Some(idx) => &abs_path[..idx],
    }
}

pub(crate) fn do_openat(dfd: i32, path: &str, flags: u32) -> SyscallResult {
    let full_path = resolve_at_path(dfd, path)?;

    let dentry = match crate::fs::resolve_path(&full_path) {
        Ok(d) => {
            if (flags & crate::fs::O_CREAT) != 0 && (flags & crate::fs::O_EXCL) != 0 {
                return Err(SyscallError::EEXIST);
            }
            if (flags & crate::fs::O_DIRECTORY) != 0 && d.inode.inode_type != InodeType::Directory {
                return Err(SyscallError::ENOTDIR);
            }
            if d.inode.inode_type == InodeType::Directory && crate::fs::can_write(flags) {
                return Err(SyscallError::EISDIR);
            }
            // Permission check on the target itself (effective IDs).
            let st = d.inode.ops.stat()?;
            let mut need = 0u32;
            if crate::fs::can_read(flags) {
                need |= R_OK;
            }
            if crate::fs::can_write(flags) {
                need |= W_OK;
            }
            check_access_stat(&st, need, true)?;
            if (flags & crate::fs::O_TRUNC) != 0 && crate::fs::can_write(flags) {
                let _ = d.inode.ops.truncate(0);
            }
            d
        }
        Err(crate::fs::vfs::types::VfsError::NotFound) if (flags & crate::fs::O_CREAT) != 0 => {
            if (flags & crate::fs::O_DIRECTORY) != 0 {
                return Err(SyscallError::ENOENT);
            }
            // Creating: require write+exec on the parent directory.
            let pst = crate::fs::stat(parent_dir_of(&full_path))?;
            check_access_stat(&pst, W_OK | X_OK, true)?;
            crate::fs::create_file(&full_path)?
        }
        Err(err) => return Err(SyscallError::from(err)),
    };

    let file_ops = dentry.inode.ops.open()?;
    let file = Arc::new(File::new(dentry, flags, file_ops));

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    // Honor O_CLOEXEC so fork+exec children don't leak descriptors.
    // Leaked pipe/file write ends keep readers from observing EOF and hang
    // drivers like g++ that fork/exec cc1plus/as/ld.
    let cloexec = if (flags & super::O_CLOEXEC) != 0 {
        crate::fs::fd::FD_CLOEXEC
    } else {
        0
    };
    let fd = proc.fd_table.alloc_with_flags(file, cloexec);

    Ok(fd as usize)
}

/// `sys_open` (SYS_OPEN = 2)
/// Open a file.
#[wrap_syscall]
pub fn sys_open(path_ptr: UserCStr, flags: u32) -> SyscallResult {
    let path = path_ptr.to_string(4096)?;
    do_openat(AT_FDCWD, &path, flags)
}

/// `sys_openat` (SYS_OPENAT = 257)
/// Open a file relative to directory descriptor.
#[wrap_syscall]
pub fn sys_openat(dfd: i32, path_ptr: UserCStr, flags: u32) -> SyscallResult {
    let path = path_ptr.to_string(4096)?;
    do_openat(dfd, &path, flags)
}
