//! sys_umask system call handler.

use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult};

/// `sys_umask` (SYS_UMASK = 95)
/// Set file mode creation mask.
#[wrap_syscall]
pub fn sys_umask(mask: u32) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let old_mask = proc.umask;
    proc.umask = mask & 0o777;

    Ok(old_mask as usize)
}
