//! sys_bind system call handler.

use super::*;
use core::mem::size_of;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_bind` (SYS_BIND = 49)
/// Bind a name to a socket.
#[wrap_syscall]
pub fn sys_bind(
    fd: i32,
    addr_ptr: UserPtr<SockAddrStorage>,
    addrlen: usize,
) -> SyscallResult {
    if addr_ptr.is_null() || addrlen < size_of::<u16>() {
        return Err(SyscallError::EINVAL);
    }

    let addr = addr_ptr.read().ok_or(SyscallError::EFAULT)?;
    let socket_arc = get_socket(fd)?;

    let mut sock = socket_arc.lock();
    sock.bind(&socket_arc, &addr, addrlen)?;

    Ok(0)
}
