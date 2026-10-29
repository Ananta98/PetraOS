// SPDX-License-Identifier: GPL-2.0

//! System calls for socket options: `setsockopt(2)`, `getsockopt(2)`.

use ostd::arch::cpu::context::UserContext;

use super::addr::get_socket;
use crate::{
    uapi::errno::{Errno, Result},
    proc::thread::Thread,
    utils::userspace::UserPtr,
};

const SOL_SOCKET: i32 = 1;
const IPPROTO_TCP: i32 = 6;

const SO_REUSEADDR: i32 = 2;
const SO_TYPE: i32 = 3;
const SO_ERROR: i32 = 4;
const SO_SNDBUF: i32 = 7;
const SO_RCVBUF: i32 = 8;
const SO_PASSCRED: i32 = 16;
const SO_PEERCRED: i32 = 17;
const TCP_NODELAY: i32 = 1;

/// System call `setsockopt(...)` (Syscall #54).
pub fn sys_setsockopt(
    fd: i32,
    level: i32,
    optname: i32,
    optval_ptr: usize,
    optlen: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let socket = get_socket(fd)?;
    let proc = Thread::proc_or_esrch()?;

    let mut val = [0u8; 4];
    if optlen > 0 && optval_ptr != 0 {
        let copy_len = (optlen as usize).min(4);
        let vmar = proc.vmspace();
        UserPtr::new(optval_ptr).read_bytes(&vmar, &mut val[..copy_len])?;
    }

    let _ = (socket, level, optname);
    Ok(0)
}

/// System call `getsockopt(...)` (Syscall #55).
pub fn sys_getsockopt(
    fd: i32,
    level: i32,
    optname: i32,
    optval_ptr: usize,
    optlen_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let socket = get_socket(fd)?;
    let proc = Thread::proc_or_esrch()?;

    if optval_ptr == 0 || optlen_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    let vmar = proc.vmspace();

    if level == SOL_SOCKET && optname == SO_PEERCRED {
        return handle_get_peercred(&socket, &vmar, optval_ptr, optlen_ptr);
    }

    let val = resolve_sockopt_val(&socket, level, optname);
    UserPtr::new(optval_ptr).write_val(&vmar, val)?;
    UserPtr::new(optlen_ptr).write_val(&vmar, 4u32)?;

    Ok(0)
}

fn handle_get_peercred(
    socket: &crate::net::socket::KernelSocket,
    vmar: &crate::vm::Vmar,
    optval_ptr: usize,
    optlen_ptr: usize,
) -> Result<usize> {
    let cred = socket.peer_cred.lock().unwrap_or(crate::uapi::cred::PeerCred {
        pid: 0,
        uid: 0,
        gid: 0,
    });
    let mut raw = [0u8; 12];
    raw[0..4].copy_from_slice(&cred.pid.to_ne_bytes());
    raw[4..8].copy_from_slice(&cred.uid.to_ne_bytes());
    raw[8..12].copy_from_slice(&cred.gid.to_ne_bytes());

    let user_len: u32 = UserPtr::new(optlen_ptr).read_val(vmar)?;
    let to_copy = (user_len as usize).min(12);
    UserPtr::new(optval_ptr).write_bytes(vmar, &raw[..to_copy])?;
    UserPtr::new(optlen_ptr).write_val(vmar, 12u32)?;
    Ok(0)
}

fn resolve_sockopt_val(socket: &crate::net::socket::KernelSocket, level: i32, optname: i32) -> i32 {
    match (level, optname) {
        (SOL_SOCKET, SO_REUSEADDR) => 1,
        (SOL_SOCKET, SO_TYPE) => socket.sock_type,
        (SOL_SOCKET, SO_ERROR) => get_socket_error(socket),
        (SOL_SOCKET, SO_RCVBUF) => 212992,
        (SOL_SOCKET, SO_SNDBUF) => 212992,
        (IPPROTO_TCP, TCP_NODELAY) => 1,
        _ => 0,
    }
}

fn get_socket_error(socket: &crate::net::socket::KernelSocket) -> i32 {
    let kind = socket.kind.lock().clone();
    if let crate::net::socket::SocketKind::Tcp(handle) = kind {
        let mut net = crate::net::interface::NET_MANAGER.lock();
        let s = net
            .get_sockets()
            .get_mut::<smoltcp::socket::tcp::Socket>(handle);
        if s.state() == smoltcp::socket::tcp::State::Closed {
            return Errno::ECONNREFUSED as i32;
        }
    }
    0
}
