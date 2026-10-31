// SPDX-License-Identifier: GPL-2.0

//! Socket status and connection state queries: `getsockname`, `getpeername`, `shutdown`.

use ostd::arch::cpu::context::UserContext;

use super::addr::{get_socket, write_sockaddr_with_domain};
use crate::{
    uapi::errno::Result,
    proc::thread::Thread,
};

/// System call `getsockname(...)` (Syscall #51).
pub fn sys_getsockname(
    fd: i32,
    addr_ptr: usize,
    addrlen_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let socket = get_socket(fd)?;
    let proc = Thread::proc_or_esrch()?;
    let endpoint = socket.getsockname()?;
    write_sockaddr_with_domain(&proc, endpoint, Some(socket.domain), addr_ptr, addrlen_ptr)?;
    Ok(0)
}

/// System call `getpeername(...)` (Syscall #52).
pub fn sys_getpeername(
    fd: i32,
    addr_ptr: usize,
    addrlen_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let socket = get_socket(fd)?;
    let proc = Thread::proc_or_esrch()?;
    let endpoint = socket.getpeername()?;
    write_sockaddr_with_domain(&proc, endpoint, Some(socket.domain), addr_ptr, addrlen_ptr)?;
    Ok(0)
}

/// System call `shutdown(...)` (Syscall #48).
pub fn sys_shutdown(fd: i32, how: i32, _ctx: &mut UserContext) -> Result<usize> {
    let socket = get_socket(fd)?;
    socket.shutdown(how)?;
    Ok(0)
}
