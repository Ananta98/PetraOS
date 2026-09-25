//! sys_shutdown system call handler.

use super::*;
use crate::syscalls::{wrap_syscall, SyscallResult};

/// `sys_shutdown` (SYS_SHUTDOWN = 48)
/// Shut down part of a full-duplex connection.
#[wrap_syscall]
pub fn sys_shutdown(fd: i32, how: i32) -> SyscallResult {
    let socket_arc = get_socket(fd)?;
    socket_arc.lock().shutdown(how)?;

    Ok(0)
}
