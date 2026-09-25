//! sys_sendto system call handler.

use super::*;
use core::mem::size_of;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_sendto` (SYS_SENDTO = 44)
/// Send a message on a socket to a specific destination.
#[wrap_syscall]
pub fn sys_sendto(
    fd: i32,
    buf_ptr: UserPtr<u8>,
    len: usize,
    flags: i32,
    dest_addr_ptr: UserPtr<SockAddrStorage>,
    addrlen: usize,
) -> SyscallResult {
    let buf_slice = buf_ptr.as_slice(len).ok_or(SyscallError::EFAULT)?;
    let socket_arc = get_socket(fd)?;

    let maybe_dest = if !dest_addr_ptr.is_null() && addrlen >= size_of::<u16>() {
        Some(dest_addr_ptr.read().ok_or(SyscallError::EFAULT)?)
    } else {
        None
    };

    let mut sock = socket_arc.lock();
    let sent = sock.sendto(buf_slice, maybe_dest.as_ref(), addrlen, flags)?;
    Ok(sent)
}
