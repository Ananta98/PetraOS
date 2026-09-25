//! System calls for creating special or generic filesystem nodes (`mknodat`).

use super::*;
use crate::syscalls::{wrap_syscall, SyscallResult, UserCStr};

/// `sys_mknodat` (SYS_MKNODAT = 259)
/// Create a special or ordinary file relative to a directory file descriptor.
#[wrap_syscall]
pub fn sys_mknodat(dfd: i32, path_ptr: UserCStr, _mode: u32, _dev: u64) -> SyscallResult {
    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    let _ = crate::fs::create_file(&full_path)?;
    Ok(0)
}
