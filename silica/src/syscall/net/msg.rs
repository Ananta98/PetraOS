// SPDX-License-Identifier: GPL-2.0

//! Advanced message transmission syscalls: `sendmsg`, `recvmsg`, `sendmmsg`, `recvmmsg`.

use ostd::arch::cpu::context::UserContext;

use super::addr::{get_socket, read_sockaddr, write_sockaddr};
use crate::{
    api::errno::Result,
    net::socket::SOCK_DGRAM,
    proc::thread::Thread,
    syscall::fs::utils::{UserPtr, read_user_iovecs},
};

/// Linux `struct msghdr` layout on x86-64 (56 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MsgHdr {
    pub msg_name: usize,
    pub msg_namelen: u32,
    pub _pad1: u32,
    pub msg_iov: usize,
    pub msg_iovlen: usize,
    pub msg_control: usize,
    pub msg_controllen: usize,
    pub msg_flags: i32,
    pub _pad2: i32,
}

impl MsgHdr {
    pub fn read_from_user(vmar: &crate::vm::vmar::Vmar, ptr: usize) -> Result<Self> {
        let mut raw = [0u8; 56];
        UserPtr::new(ptr).read_bytes(vmar, &mut raw)?;
        Ok(Self {
            msg_name: usize::from_ne_bytes(raw[0..8].try_into().unwrap()),
            msg_namelen: u32::from_ne_bytes(raw[8..12].try_into().unwrap()),
            _pad1: 0,
            msg_iov: usize::from_ne_bytes(raw[16..24].try_into().unwrap()),
            msg_iovlen: usize::from_ne_bytes(raw[24..32].try_into().unwrap()),
            msg_control: usize::from_ne_bytes(raw[32..40].try_into().unwrap()),
            msg_controllen: usize::from_ne_bytes(raw[40..48].try_into().unwrap()),
            msg_flags: i32::from_ne_bytes(raw[48..52].try_into().unwrap()),
            _pad2: 0,
        })
    }

    pub fn write_results_to_user(&self, vmar: &crate::vm::vmar::Vmar, ptr: usize) -> Result<()> {
        UserPtr::new(ptr + 8).write_bytes(vmar, &self.msg_namelen.to_ne_bytes())?;
        UserPtr::new(ptr + 40).write_bytes(vmar, &self.msg_controllen.to_ne_bytes())?;
        UserPtr::new(ptr + 48).write_bytes(vmar, &self.msg_flags.to_ne_bytes())?;
        Ok(())
    }
}

/// System call `sendmsg(int sockfd, const struct msghdr *msg, int flags)` (Syscall #46).
pub fn sys_sendmsg(fd: i32, msg_ptr: usize, flags: i32, _ctx: &mut UserContext) -> Result<usize> {
    let socket = get_socket(fd)?;
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let msghdr = MsgHdr::read_from_user(&vmar, msg_ptr)?;

    let dest = if msghdr.msg_name != 0 && msghdr.msg_namelen > 0 {
        Some(read_sockaddr(&proc, msghdr.msg_name, msghdr.msg_namelen)?)
    } else {
        None
    };

    let iovecs = read_user_iovecs(&vmar, msghdr.msg_iov, msghdr.msg_iovlen)?;
    if iovecs.is_empty() {
        return Ok(0);
    }

    if socket.sock_type == SOCK_DGRAM || dest.is_some() {
        // Datagram sockets transmit a single packet from gathered buffers
        let total_len: usize = iovecs.iter().map(|iov| iov.iov_len).sum();
        let mut kbuf = alloc::vec![0u8; total_len.min(65536)];
        let mut offset = 0;
        for iov in &iovecs {
            if iov.iov_len == 0 || offset >= kbuf.len() {
                continue;
            }
            let to_read = iov.iov_len.min(kbuf.len() - offset);
            UserPtr::new(iov.iov_base).read_bytes(&vmar, &mut kbuf[offset..offset + to_read])?;
            offset += to_read;
        }

        if dest.is_some() {
            socket.sendto(&kbuf[..offset], dest, flags)
        } else {
            socket.send(&kbuf[..offset], flags)
        }
    } else {
        // Stream socket: send sequentially through iovecs
        let mut total_sent = 0;
        for iov in &iovecs {
            if iov.iov_len == 0 {
                continue;
            }
            let mut chunk = alloc::vec![0u8; iov.iov_len.min(65536)];
            let chunk_len = chunk.len();
            UserPtr::new(iov.iov_base).read_bytes(&vmar, &mut chunk)?;

            let n = socket.send(&chunk[..chunk_len], flags)?;
            total_sent += n;
            if n < chunk_len {
                break;
            }
        }
        Ok(total_sent)
    }
}

/// System call `recvmsg(int sockfd, struct msghdr *msg, int flags)` (Syscall #47).
pub fn sys_recvmsg(fd: i32, msg_ptr: usize, flags: i32, _ctx: &mut UserContext) -> Result<usize> {
    let socket = get_socket(fd)?;
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let mut msghdr = MsgHdr::read_from_user(&vmar, msg_ptr)?;
    let iovecs = read_user_iovecs(&vmar, msghdr.msg_iov, msghdr.msg_iovlen)?;
    if iovecs.is_empty() {
        return Ok(0);
    }

    if socket.sock_type == SOCK_DGRAM {
        let total_cap: usize = iovecs.iter().map(|iov| iov.iov_len).sum();
        let mut kbuf = alloc::vec![0u8; total_cap.min(65536)];
        let (nrecv, endpoint_opt) = socket.recvfrom(&mut kbuf, flags)?;

        // Scatter received datagram bytes into user iovecs
        let mut offset = 0;
        for iov in &iovecs {
            if offset >= nrecv {
                break;
            }
            let to_write = iov.iov_len.min(nrecv - offset);
            if to_write > 0 {
                UserPtr::new(iov.iov_base).write_bytes(&vmar, &kbuf[offset..offset + to_write])?;
                offset += to_write;
            }
        }

        if msghdr.msg_name != 0
            && msghdr.msg_namelen > 0
            && let Some(endpoint) = endpoint_opt
        {
            let _ = write_sockaddr(&proc, endpoint, msghdr.msg_name, msg_ptr + 8);
        }

        msghdr.msg_controllen = 0;
        msghdr.msg_flags = 0;
        msghdr.write_results_to_user(&vmar, msg_ptr)?;

        Ok(nrecv)
    } else {
        let mut total_recv = 0;
        for iov in &iovecs {
            if iov.iov_len == 0 {
                continue;
            }
            let mut chunk = alloc::vec![0u8; iov.iov_len.min(65536)];
            let n = socket.recv(&mut chunk, flags)?;
            if n > 0 {
                UserPtr::new(iov.iov_base).write_bytes(&vmar, &chunk[..n])?;
                total_recv += n;
            }
            if n < chunk.len() {
                break;
            }
        }

        if msghdr.msg_name != 0
            && msghdr.msg_namelen > 0
            && let Ok(peer) = socket.getpeername()
        {
            let _ = write_sockaddr(&proc, peer, msghdr.msg_name, msg_ptr + 8);
        }

        msghdr.msg_controllen = 0;
        msghdr.msg_flags = 0;
        msghdr.write_results_to_user(&vmar, msg_ptr)?;

        Ok(total_recv)
    }
}

/// System call `sendmmsg(int sockfd, struct mmsghdr *msgvec, unsigned int vlen, int flags)` (Syscall #307).
pub fn sys_sendmmsg(
    fd: i32,
    msgvec_ptr: usize,
    vlen: u32,
    flags: i32,
    ctx: &mut UserContext,
) -> Result<usize> {
    transfer_mmsg(msgvec_ptr, vlen, ctx, &mut |entry_ptr, ctx| {
        sys_sendmsg(fd, entry_ptr, flags, ctx)
    })
}

/// System call `recvmmsg(int sockfd, struct mmsghdr *msgvec, unsigned int vlen, int flags, struct timespec *timeout)` (Syscall #299).
pub fn sys_recvmmsg(
    fd: i32,
    msgvec_ptr: usize,
    vlen: u32,
    flags: i32,
    _timeout_ptr: usize,
    ctx: &mut UserContext,
) -> Result<usize> {
    transfer_mmsg(msgvec_ptr, vlen, ctx, &mut |entry_ptr, ctx| {
        sys_recvmsg(fd, entry_ptr, flags, ctx)
    })
}

/// Shared `*mmsg` driver: iterates `struct mmsghdr` entries, invoking one
/// `sendmsg`/`recvmsg` per entry and recording its return length. Partial
/// success is reported as completed entries; the first failure with nothing
/// yet completed is propagated.
fn transfer_mmsg(
    msgvec_ptr: usize,
    vlen: u32,
    ctx: &mut UserContext,
    transfer: &mut dyn FnMut(usize, &mut UserContext) -> Result<usize>,
) -> Result<usize> {
    let count = (vlen as usize).min(1024);
    if count == 0 {
        return Ok(0);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let mut done = 0;
    for i in 0..count {
        let entry_ptr = msgvec_ptr + i * 64;
        match transfer(entry_ptr, ctx) {
            Ok(n) => {
                UserPtr::new(entry_ptr + 56).write_val(&vmar, n as u32)?;
                done += 1;
            }
            Err(e) => {
                if done > 0 {
                    break;
                }
                return Err(e);
            }
        }
    }

    Ok(done)
}
