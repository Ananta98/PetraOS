//! sys_chmod system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_chmod` (SYS_CHMOD = 90)
pub fn sys_chmod(frame: &mut SyscallFrame) -> SyscallResult {
    let mode = frame.arg2() as u32;

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;

    let st = crate::fs::stat(&full_path)?;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let creds = { Arc::clone(&proc_arc.lock().creds) };
    if creds.euid != 0 && creds.euid != st.uid {
        return Err(SyscallError::EPERM);
    }

    crate::fs::chmod(&full_path, mode)?;
    Ok(0)
}
