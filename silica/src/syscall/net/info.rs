// SPDX-License-Identifier: GPL-2.0

//! Socket status and connection state queries: `getsockname`, `getpeername`, `shutdown`.

use ostd::arch::cpu::context::UserContext;

use super::addr::{get_socket, write_sockaddr};
use crate::{
    api::errno::{Errno, Result},
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
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let endpoint = socket.getsockname()?;
    write_sockaddr(&proc, endpoint, addr_ptr, addrlen_ptr)?;
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
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let endpoint = socket.getpeername()?;
    write_sockaddr(&proc, endpoint, addr_ptr, addrlen_ptr)?;
    Ok(0)
}

/// System call `shutdown(...)` (Syscall #48).
pub fn sys_shutdown(fd: i32, how: i32, _ctx: &mut UserContext) -> Result<usize> {
    let socket = get_socket(fd)?;
    socket.shutdown(how)?;
    Ok(0)
}
