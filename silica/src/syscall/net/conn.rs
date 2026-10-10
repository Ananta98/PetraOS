// SPDX-License-Identifier: GPL-2.0

//! Connection management syscalls: `bind`, `connect`, `listen`, `accept`, `accept4`.

use ostd::arch::cpu::context::UserContext;

use super::{
    addr::{get_socket, read_sockaddr, write_sockaddr},
    socket::{SOCK_CLOEXEC, SOCK_NONBLOCK},
};
use crate::{
    api::errno::{Errno, Result},
    fs::{fd::FdFlags, socket::create_socket_file},
    proc::thread::Thread,
};

/// System call `bind(int sockfd, const struct sockaddr *addr, socklen_t addrlen)` (Syscall #49).
pub fn sys_bind(
    fd: i32,
    addr_ptr: usize,
    addrlen: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let socket = get_socket(fd)?;
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let endpoint = read_sockaddr(&proc, addr_ptr, addrlen)?;
    socket.bind(endpoint)?;
    Ok(0)
}

/// System call `connect(int sockfd, const struct sockaddr *addr, socklen_t addrlen)` (Syscall #42).
pub fn sys_connect(
    fd: i32,
    addr_ptr: usize,
    addrlen: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let socket = get_socket(fd)?;
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let endpoint = read_sockaddr(&proc, addr_ptr, addrlen)?;
    socket.connect(endpoint)?;
    Ok(0)
}

/// System call `listen(int sockfd, int backlog)` (Syscall #50).
pub fn sys_listen(
    fd: i32,
    backlog: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let socket = get_socket(fd)?;
    socket.listen(backlog)?;
    Ok(0)
}

/// System call `accept4(int sockfd, struct sockaddr *addr, socklen_t *addrlen, int flags)` (Syscall #288).
pub fn sys_accept4(
    fd: i32,
    addr_ptr: usize,
    addrlen_ptr: usize,
    flags: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if (flags & !(SOCK_NONBLOCK | SOCK_CLOEXEC)) != 0 {
        return Err(Errno::EINVAL);
    }

    let socket = get_socket(fd)?;
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;

    let (client_sock, remote_endpoint) = socket.accept()?;

    if (flags & SOCK_NONBLOCK) != 0 {
        *client_sock.nonblocking.lock() = true;
    }

    if addr_ptr != 0 && addrlen_ptr != 0 {
        let _ = write_sockaddr(&proc, remote_endpoint, addr_ptr, addrlen_ptr);
    }

    let file = create_socket_file(client_sock);
    let mut fd_flags = FdFlags::empty();
    if (flags & SOCK_CLOEXEC) != 0 {
        fd_flags |= FdFlags::FD_CLOEXEC;
    }

    let new_fd = proc.fd_table.lock().alloc_fd(file, fd_flags)?;
    Ok(new_fd as usize)
}

/// System call `accept(int sockfd, struct sockaddr *addr, socklen_t *addrlen)` (Syscall #43).
pub fn sys_accept(
    fd: i32,
    addr_ptr: usize,
    addrlen_ptr: usize,
    ctx: &mut UserContext,
) -> Result<usize> {
    sys_accept4(fd, addr_ptr, addrlen_ptr, 0, ctx)
}
