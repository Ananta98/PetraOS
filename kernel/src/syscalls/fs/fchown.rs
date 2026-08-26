//! sys_fchown system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_fchown` (SYS_FCHOWN = 93)
pub fn sys_fchown(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let uid = frame.arg2() as u32;
    let gid = frame.arg3() as u32;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    let creds = Arc::clone(&proc.creds);
    drop(proc);

    let st = file.dentry.inode.ops.stat()?;
    let (uid, gid) = effective_owner(&st, uid, gid);

    if creds.euid != 0 {
        if uid != st.uid || (gid != st.gid && gid != creds.gid && gid != creds.egid) {
            return Err(SyscallError::EPERM);
        }
    }

    file.ops
        .chown(uid, gid)
        .or_else(|_| file.dentry.inode.ops.chown(uid, gid))?;
    Ok(0)
}
