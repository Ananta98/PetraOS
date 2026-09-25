//! sys_accept system call handler.

use super::*;
use crate::syscalls::{wrap_syscall, SyscallResult, UserPtr};

/// `sys_accept` (SYS_ACCEPT = 43)
/// Accept a connection on a socket.
#[wrap_syscall]
pub fn sys_accept(
    fd: i32,
    addr_ptr: UserPtr<SockAddrStorage>,
    addrlen_ptr: UserPtr<SockLen>,
) -> SyscallResult {
    accept_internal(fd, addr_ptr, addrlen_ptr, 0)
}

/// `sys_accept4` (SYS_ACCEPT4 = 288)
/// Accept a connection on a socket with flags (SOCK_NONBLOCK, SOCK_CLOEXEC).
#[wrap_syscall]
pub fn sys_accept4(
    fd: i32,
    addr_ptr: UserPtr<SockAddrStorage>,
    addrlen_ptr: UserPtr<SockLen>,
    flags: i32,
) -> SyscallResult {
    accept_internal(fd, addr_ptr, addrlen_ptr, flags)
}
