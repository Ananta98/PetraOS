//! sys_lchown system call handler.

use super::*;
use crate::syscalls::{SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_lchown` (SYS_LCHOWN = 94)
pub fn sys_lchown(frame: &mut SyscallFrame) -> SyscallResult {
    let uid = frame.arg2() as u32;
    let gid = frame.arg3() as u32;

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let dentry = crate::fs::resolve_path_nofollow(&full_path)?;
    let st = dentry.inode.ops.stat()?;
    let (uid, gid) = effective_owner(&st, uid, gid);
    dentry.inode.ops.chown(uid, gid)?;
    Ok(0)
}
