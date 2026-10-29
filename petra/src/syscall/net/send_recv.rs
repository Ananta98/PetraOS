// SPDX-License-Identifier: GPL-2.0

//! System calls for basic socket I/O: `sendto(2)`, `recvfrom(2)`.

use ostd::arch::cpu::context::UserContext;

use super::addr::{get_socket, read_sockaddr, write_sockaddr};
use crate::{
    uapi::errno::Result,
    proc::thread::Thread,
    utils::userspace::UserPtr,
};

/// System call `sendto(...)` (Syscall #44).
pub fn sys_sendto(
    fd: i32,
    buf_ptr: usize,
    len: usize,
    flags: i32,
    dest_ptr: usize,
    addrlen: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if len == 0 {
        return Ok(0);
    }
    let socket = get_socket(fd)?;
    let proc = Thread::proc_or_esrch()?;

    let mut kbuf = alloc::vec![0u8; len.min(65536)];
    let copy_len = kbuf.len();

    let vmar = proc.vmspace();
    UserPtr::new(buf_ptr).read_bytes(&vmar, &mut kbuf)?;

    let endpoint = if dest_ptr != 0 && addrlen > 0 {
        Some(read_sockaddr(&proc, dest_ptr, addrlen)?)
    } else {
        None
    };

    if endpoint.is_some() {
        socket.sendto(&kbuf[..copy_len], endpoint, flags)
    } else {
        socket.send(&kbuf[..copy_len], flags)
    }
}

/// System call `recvfrom(...)` (Syscall #45).
pub fn sys_recvfrom(
    fd: i32,
    buf_ptr: usize,
    len: usize,
    flags: i32,
    src_ptr: usize,
    addrlen_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if len == 0 {
        return Ok(0);
    }
    let socket = get_socket(fd)?;
    let proc = Thread::proc_or_esrch()?;

    let mut kbuf = alloc::vec![0u8; len.min(65536)];
    let (nrecv, endpoint_opt) = if src_ptr != 0 && addrlen_ptr != 0 {
        socket.recvfrom(&mut kbuf, flags)?
    } else {
        let n = socket.recv(&mut kbuf, flags)?;
        (n, None)
    };

    if nrecv > 0 {
        let vmar = proc.vmspace();
        UserPtr::new(buf_ptr).write_bytes(&vmar, &kbuf[..nrecv])?;
    }

    if src_ptr != 0
        && addrlen_ptr != 0
        && let Some(endpoint) = endpoint_opt
    {
        let _ = write_sockaddr(&proc, endpoint, src_ptr, addrlen_ptr);
    }

    Ok(nrecv)
}
