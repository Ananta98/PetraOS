//! sys_setsockopt system call handler.

use super::*;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_setsockopt` (SYS_SETSOCKOPT = 54)
/// Set options on sockets.
#[wrap_syscall]
pub fn sys_setsockopt(
    fd: i32,
    _level: i32,
    optname: i32,
    optval_ptr: UserPtr<i32>,
    _optlen: usize,
) -> SyscallResult {
    let _socket_arc = get_socket(fd)?;

    if !optval_ptr.is_null() {
        let val = optval_ptr.read().ok_or(SyscallError::EFAULT)?;
        match optname {
            SO_REUSEADDR | SO_REUSEPORT | SO_KEEPALIVE | TCP_NODELAY => {
                log::trace!("[setsockopt] optname {} set to {}", optname, val);
            }
            SO_RCVTIMEO | SO_SNDTIMEO => {
                log::trace!("[setsockopt] timeout optname {} configured", optname);
            }
            _ => {}
        }
    }

    Ok(0)
}
