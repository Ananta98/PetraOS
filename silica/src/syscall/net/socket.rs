// SPDX-License-Identifier: GPL-2.0

//! System calls for creating sockets: `socket(2)`, `socketpair(2)`.

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    fs::{fd::FdFlags, socket::create_socket_file},
    net::socket::{
        KernelSocket, AF_INET, AF_INET6, AF_NETLINK, AF_UNIX, IPPROTO_IP, IPPROTO_TCP,
        IPPROTO_UDP, SOCK_DGRAM, SOCK_RAW, SOCK_STREAM,
    },
    proc::thread::Thread,
    syscall::fs::utils::UserPtr,
};

pub const SOCK_NONBLOCK: i32 = 0o4000;
pub const SOCK_CLOEXEC: i32 = 0o2000000;

/// System call `socket(int domain, int type, int protocol)` (Syscall #41).
pub fn sys_socket(
    domain: i32,
    sock_type: i32,
    protocol: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;

    let nonblocking = (sock_type & SOCK_NONBLOCK) != 0;
    let cloexec = (sock_type & SOCK_CLOEXEC) != 0;
    let base_type = sock_type & !(SOCK_NONBLOCK | SOCK_CLOEXEC);

    let socket = match domain {
        AF_INET | AF_INET6 => match base_type {
            SOCK_STREAM => {
                if protocol != IPPROTO_IP && protocol != IPPROTO_TCP {
                    crate::return_errno!(EPROTONOSUPPORT, "unsupported protocol for stream socket");
                }
                KernelSocket::new_tcp(domain)?
            }
            SOCK_DGRAM => {
                if protocol != IPPROTO_IP && protocol != IPPROTO_UDP {
                    crate::return_errno!(EPROTONOSUPPORT, "unsupported protocol for dgram socket");
                }
                KernelSocket::new_udp(domain)?
            }
            _ => crate::return_errno!(ESOCKTNOSUPPORT, "unsupported socket type"),
        },
        AF_UNIX => {
            if protocol != 0 && protocol != IPPROTO_IP {
                crate::return_errno!(EPROTONOSUPPORT, "unsupported protocol for unix socket");
            }
            if base_type != SOCK_STREAM && base_type != SOCK_DGRAM {
                crate::return_errno!(ESOCKTNOSUPPORT, "unsupported socket type for unix domain");
            }
            KernelSocket::new_unix(base_type)?
        }
        AF_NETLINK => {
            if base_type != SOCK_RAW && base_type != SOCK_DGRAM {
                crate::return_errno!(ESOCKTNOSUPPORT, "unsupported socket type for netlink");
            }
            KernelSocket::new_netlink(protocol)?
        }
        _ => crate::return_errno!(EAFNOSUPPORT, "unsupported socket domain"),
    };

    if nonblocking {
        *socket.nonblocking.lock() = true;
    }

    let file = create_socket_file(socket);
    let mut fd_flags = FdFlags::empty();
    if cloexec {
        fd_flags |= FdFlags::FD_CLOEXEC;
    }

    let fd = proc.fd_table.lock().alloc_fd(file, fd_flags)?;
    Ok(fd as usize)
}

/// System call `socketpair(int domain, int type, int protocol, int sv[2])` (Syscall #53).
pub fn sys_socketpair(
    domain: i32,
    sock_type: i32,
    protocol: i32,
    sv_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if sv_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    // Linux only supports AF_UNIX for socketpair
    if domain != AF_UNIX {
        crate::return_errno!(EAFNOSUPPORT, "socketpair only supported for AF_UNIX domain");
    }

    let nonblocking = (sock_type & SOCK_NONBLOCK) != 0;
    let cloexec = (sock_type & SOCK_CLOEXEC) != 0;
    let base_type = sock_type & !(SOCK_NONBLOCK | SOCK_CLOEXEC);

    if base_type != SOCK_STREAM && base_type != SOCK_DGRAM {
        crate::return_errno!(ESOCKTNOSUPPORT, "unsupported socketpair socket type");
    }

    if protocol != IPPROTO_IP && protocol != 0 {
        crate::return_errno!(EPROTONOSUPPORT, "unsupported socketpair protocol");
    }

    let (sock_a, sock_b) = KernelSocket::new_unix_pair(base_type)?;

    if nonblocking {
        *sock_a.nonblocking.lock() = true;
        *sock_b.nonblocking.lock() = true;
    }

    let file_a = create_socket_file(sock_a);
    let file_b = create_socket_file(sock_b);

    let mut fd_flags = FdFlags::empty();
    if cloexec {
        fd_flags |= FdFlags::FD_CLOEXEC;
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let mut table = proc.fd_table.lock();

    let fd0 = table.alloc_fd(file_a, fd_flags)?;
    let fd1 = match table.alloc_fd(file_b, fd_flags) {
        Ok(fd) => fd,
        Err(e) => {
            let _ = table.close(fd0);
            return Err(e);
        }
    };
    drop(table);

    let fds: [i32; 2] = [fd0, fd1];
    let mut raw_bytes = [0u8; 8];
    raw_bytes[0..4].copy_from_slice(&fds[0].to_ne_bytes());
    raw_bytes[4..8].copy_from_slice(&fds[1].to_ne_bytes());

    UserPtr::new(sv_ptr).write_bytes(&proc.vmspace(), &raw_bytes)?;
    Ok(0)
}
