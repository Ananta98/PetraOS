//! sys_fchmod system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_fchmod` (SYS_FCHMOD = 91)
pub fn sys_fchmod(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let mode = frame.arg2() as u32;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    let creds = Arc::clone(&proc.creds);
    drop(proc);

    let st = file.ops.stat().or_else(|_| file.dentry.inode.ops.stat())?;
    if creds.euid != 0 && creds.euid != st.uid {
        return Err(SyscallError::EPERM);
    }

    file.ops
        .chmod(mode)
        .or_else(|_| file.dentry.inode.ops.chmod(mode))?;
    Ok(0)
}
