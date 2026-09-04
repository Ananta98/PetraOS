//! sys_socket system call handler.

use super::*;
use alloc::sync::Arc;
use crate::arch::syscall::SyscallFrame;
use crate::fs::create_socket_file;
use crate::fs::fd::FD_CLOEXEC;
use crate::net::socket::Socket;
use crate::sync::Mutex;
use crate::syscalls::{SyscallError, SyscallResult};


/// `sys_socket` (SYS_SOCKET = 41)
/// Create an endpoint for communication.
pub fn sys_socket(frame: &mut SyscallFrame) -> SyscallResult {
    let domain = frame.arg1() as i32;
    let socket_type = frame.arg2() as i32;
    let protocol = frame.arg3() as i32;

    let socket = Socket::new(domain, socket_type, protocol)?;
    let socket_arc = Arc::new(Mutex::new(socket));

    let file_flags = if (socket_type & SOCK_NONBLOCK) != 0 {
        crate::fs::vfs::types::O_NONBLOCK | crate::fs::vfs::types::O_RDWR
    } else {
        crate::fs::vfs::types::O_RDWR
    };

    let file = create_socket_file(socket_arc, file_flags);

    let desc_flags = if (socket_type & SOCK_CLOEXEC) != 0 {
        FD_CLOEXEC
    } else {
        0
    };

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let fd = proc.fd_table.alloc_with_flags(file, desc_flags);

    Ok(fd as usize)
}
