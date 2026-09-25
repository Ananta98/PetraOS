//! sys_getsockopt system call handler.

use super::*;
use core::mem::size_of;
use crate::net::socket::Socket;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_getsockopt` (SYS_GETSOCKOPT = 55)
/// Get options on sockets.
#[wrap_syscall]
pub fn sys_getsockopt(
    fd: i32,
    _level: i32,
    optname: i32,
    optval_ptr: UserPtr<i32>,
    optlen_ptr: UserPtr<SockLen>,
) -> SyscallResult {
    if optval_ptr.is_null() || optlen_ptr.is_null() {
        return Err(SyscallError::EFAULT);
    }

    let socket_arc = get_socket(fd)?;

    match optname {
        SO_TYPE => {
            let sock_type = match &*socket_arc.lock() {
                Socket::Tcp(_) => SOCK_STREAM,
                Socket::Udp(_) => SOCK_DGRAM,
                Socket::Raw(_) => SOCK_RAW,
                Socket::Unix(u) => u.lock().socket_type,
            };
            optval_ptr.write(sock_type).ok_or(SyscallError::EFAULT)?;
            optlen_ptr
                .write(size_of::<i32>() as SockLen)
                .ok_or(SyscallError::EFAULT)?;
        }
        SO_ERROR => {
            optval_ptr.write(0).ok_or(SyscallError::EFAULT)?;
            optlen_ptr
                .write(size_of::<i32>() as SockLen)
                .ok_or(SyscallError::EFAULT)?;
        }
        _ => {
            optval_ptr.write(0).ok_or(SyscallError::EFAULT)?;
            optlen_ptr
                .write(size_of::<i32>() as SockLen)
                .ok_or(SyscallError::EFAULT)?;
        }
    }

    Ok(0)
}
