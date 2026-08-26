//! sys_socketpair system call handler.

use super::*;
use alloc::sync::Arc;
use core::mem::size_of;
use crate::arch::syscall::SyscallFrame;
use crate::fs::create_socket_file;
use crate::fs::fd::FD_CLOEXEC;
use crate::fs::vfs::types::*;
use crate::net::socket::{Socket, UnixSocket};
use crate::net::types::*;
use crate::sync::Mutex;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};


/// `sys_socketpair` (SYS_SOCKETPAIR = 53)
/// Create a pair of connected sockets.
pub fn sys_socketpair(frame: &mut SyscallFrame) -> SyscallResult {
    let domain = frame.arg1() as i32;
    let socket_type = frame.arg2() as i32;
    let _protocol = frame.arg3() as i32;
    let sv_ptr = UserPtr::<[i32; 2]>::from_u64(frame.arg4());

    if domain as u16 != AF_UNIX {
        return Err(SyscallError::EAFNOSUPPORT);
    }

    let actual_type = socket_type & SOCK_TYPE_MASK;
    let nonblocking = (socket_type & SOCK_NONBLOCK) != 0;
    let cloexec = (socket_type & SOCK_CLOEXEC) != 0;

    let (sock_a, sock_b) = UnixSocket::create_pair(actual_type, nonblocking);

    let file_flags = if nonblocking {
        O_NONBLOCK | O_RDWR
    } else {
        O_RDWR
    };

    let desc_flags = if cloexec { FD_CLOEXEC } else { 0 };

    let file_a = create_socket_file(Arc::new(Mutex::new(Socket::Unix(sock_a))), file_flags);
    let file_b = create_socket_file(Arc::new(Mutex::new(Socket::Unix(sock_b))), file_flags);

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let fd_a = proc.fd_table.alloc_with_flags(file_a, desc_flags);
    let fd_b = proc.fd_table.alloc_with_flags(file_b, desc_flags);

    sv_ptr.write([fd_a, fd_b]).ok_or(SyscallError::EFAULT)?;

    Ok(0)
}
