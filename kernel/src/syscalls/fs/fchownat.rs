//! sys_fchownat system call handler.

use super::*;
use crate::syscalls::{SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_fchownat` (SYS_FCHOWNAT = 260)
pub fn sys_fchownat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let uid = frame.arg3() as u32;
    let gid = frame.arg4() as u32;
    let flags = frame.arg5() as i32;

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    if (flags & crate::fs::AT_SYMLINK_NOFOLLOW) != 0 {
        let dentry = crate::fs::resolve_path_nofollow(&full_path)?;
        let st = dentry.inode.ops.stat()?;
        let (uid, gid) = effective_owner(&st, uid, gid);
        dentry.inode.ops.chown(uid, gid)?;
    } else {
        let st = crate::fs::stat(&full_path)?;
        let (uid, gid) = effective_owner(&st, uid, gid);
        crate::fs::chown(&full_path, uid, gid)?;
    }
    Ok(0)
}
