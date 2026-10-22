// SPDX-License-Identifier: GPL-2.0

//! System calls for socket options: `setsockopt(2)`, `getsockopt(2)`.

use ostd::arch::cpu::context::UserContext;

use super::addr::get_socket;
use crate::{
    uapi::errno::{Errno, Result},
    proc::thread::Thread,
    utils::userspace::UserPtr,
};

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

    let val: i32 = match (level, optname) {
        (1, 2) => 1,                // SO_REUSEADDR
        (1, 3) => socket.sock_type, // SO_TYPE
        (1, 4) => {
            // SO_ERROR
            let kind = socket.kind.lock().clone();
            if let crate::net::socket::SocketKind::Tcp(handle) = kind {
                let mut net = crate::net::interface::NET_MANAGER.lock();
                let s = net
                    .get_sockets()
                    .get_mut::<smoltcp::socket::tcp::Socket>(handle);
                if s.state() == smoltcp::socket::tcp::State::Closed {
                    Errno::ECONNREFUSED as i32
                } else {
                    0
                }
            } else {
                0
            }
        }
        (6, 1) => 1, // TCP_NODELAY
        _ => 0,
    };

    let vmar = proc.vmspace();
    UserPtr::new(optval_ptr).write_val(&vmar, val)?;
    UserPtr::new(optlen_ptr).write_val(&vmar, 4u32)?;

    Ok(0)
}
