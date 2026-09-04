//! sys_umask system call handler.

use crate::syscalls::{SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;


/// `sys_umask` (SYS_UMASK = 95)
/// Set file mode creation mask.
pub fn sys_umask(frame: &mut SyscallFrame) -> SyscallResult {
    let mask = frame.arg1() as u32;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let old_mask = proc.umask;
    proc.umask = mask & 0o777;

    Ok(old_mask as usize)
}
