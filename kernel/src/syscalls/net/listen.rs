//! sys_listen system call handler.

use super::*;
use crate::syscalls::{wrap_syscall, SyscallResult};

/// `sys_listen` (SYS_LISTEN = 50)
/// Listen for connections on a socket.
#[wrap_syscall]
pub fn sys_listen(fd: i32, backlog: usize) -> SyscallResult {
    let socket_arc = get_socket(fd)?;
    socket_arc.lock().listen(backlog)?;

    Ok(0)
}
