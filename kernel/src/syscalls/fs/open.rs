//! System calls for opening and creating files (`open`, `openat`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::InodeType;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr};
use alloc::sync::Arc;

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
            if (flags & crate::fs::O_TRUNC) != 0 && crate::fs::can_write(flags) {
                let _ = d.inode.ops.truncate(0);
            }
            d
        }
        Err(crate::fs::vfs::types::VfsError::NotFound) if (flags & crate::fs::O_CREAT) != 0 => {
            if (flags & crate::fs::O_DIRECTORY) != 0 {
                return Err(SyscallError::ENOENT);
            }
            crate::fs::create_file(&full_path)?
        }
        Err(err) => return Err(SyscallError::from(err)),
    };

    let file_ops = dentry.inode.ops.open()?;
    let file = Arc::new(File::new(dentry, flags, file_ops));

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let fd = proc.fd_table.alloc(file);

    Ok(fd as usize)
}

/// `sys_open` (SYS_OPEN = 2)
/// Open a file.
pub fn sys_open(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = UserCStr::from_u64(frame.arg1());
    let flags = frame.arg2() as u32;

    let path = path_ptr.to_string(256)?;
    do_openat(AT_FDCWD, &path, flags)
}

/// `sys_openat` (SYS_OPENAT = 257)
/// Open a file relative to directory descriptor.
pub fn sys_openat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = UserCStr::from_u64(frame.arg2());
    let flags = frame.arg3() as u32;

    let path = path_ptr.to_string(256)?;
    do_openat(dfd, &path, flags)
}
