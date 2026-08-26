//! sys_chown system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserCStr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_chown` (SYS_CHOWN = 92)
pub fn sys_chown(frame: &mut SyscallFrame) -> SyscallResult {
    let uid = frame.arg2() as u32;
    let gid = frame.arg3() as u32;

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let st = crate::fs::stat(&full_path)?;
    let (uid, gid) = effective_owner(&st, uid, gid);

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let creds = { Arc::clone(&proc_arc.lock().creds) };

    if creds.euid != 0 {
        if uid != st.uid || (gid != st.gid && gid != creds.gid && gid != creds.egid) {
            return Err(SyscallError::EPERM);
        }
    }

    crate::fs::chown(&full_path, uid, gid)?;
    Ok(0)
}
