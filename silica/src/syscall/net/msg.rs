// SPDX-License-Identifier: GPL-2.0

//! Advanced message transmission syscalls: `sendmsg`, `recvmsg`, `sendmmsg`, `recvmmsg`.

use ostd::arch::cpu::context::UserContext;

use super::addr::{get_socket, read_sockaddr, write_sockaddr};
use crate::{
    uapi::errno::Result,
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

    let attached_files = parse_control_files(&proc, &vmar, msghdr.msg_control, msghdr.msg_controllen)?;

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

        socket.sendto_with_ancillary(&kbuf[..offset], dest, attached_files, flags)
    } else {
        // Stream socket: send sequentially through iovecs
        let mut total_sent = 0;
        let mut files_to_send = attached_files;
        for iov in &iovecs {
            if iov.iov_len == 0 {
                continue;
            }
            let mut chunk = alloc::vec![0u8; iov.iov_len.min(65536)];
            let chunk_len = chunk.len();
            UserPtr::new(iov.iov_base).read_bytes(&vmar, &mut chunk)?;

            let n = if !files_to_send.is_empty() {
                socket.send_with_ancillary(&chunk[..chunk_len], core::mem::take(&mut files_to_send), flags)?
            } else {
                socket.send(&chunk[..chunk_len], flags)?
            };
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
        let (nrecv, endpoint_opt, received_files) = socket.recvfrom_with_ancillary(&mut kbuf, flags)?;

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

        msghdr.msg_flags = 0;
        write_control_files(&proc, &vmar, &mut msghdr, received_files)?;
        msghdr.write_results_to_user(&vmar, msg_ptr)?;

        Ok(nrecv)
    } else {
        let mut total_recv = 0;
        let mut all_received_files = alloc::vec::Vec::new();
        for (i, iov) in iovecs.iter().enumerate() {
            if iov.iov_len == 0 {
                continue;
            }
            let mut chunk = alloc::vec![0u8; iov.iov_len.min(65536)];
            let n = if i == 0 {
                let (received_bytes, files) = socket.recv_with_ancillary(&mut chunk, flags)?;
                all_received_files = files;
                received_bytes
            } else {
                socket.recv(&mut chunk, flags)?
            };
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

        msghdr.msg_flags = 0;
        write_control_files(&proc, &vmar, &mut msghdr, all_received_files)?;
        msghdr.write_results_to_user(&vmar, msg_ptr)?;

        Ok(total_recv)
    }
}

/// Reads attached file descriptors from `msghdr.msg_control` carrying `SCM_RIGHTS`.
fn parse_control_files(
    proc: &alloc::sync::Arc<crate::proc::Proc>,
    vmar: &alloc::sync::Arc<crate::vm::Vmar>,
    control_ptr: usize,
    control_len: usize,
) -> Result<alloc::vec::Vec<alloc::sync::Arc<crate::fs::vfs::File>>> {
    let mut attached_files = alloc::vec::Vec::new();
    if control_ptr == 0 || control_len < 16 {
        return Ok(attached_files);
    }

    let mut offset = 0;
    while offset + 16 <= control_len {
        let mut hdr = [0u8; 16];
        UserPtr::new(control_ptr + offset).read_bytes(vmar, &mut hdr)?;
        let cmsg_len = usize::from_ne_bytes(hdr[0..8].try_into().unwrap());
        let cmsg_level = i32::from_ne_bytes(hdr[8..12].try_into().unwrap());
        let cmsg_type = i32::from_ne_bytes(hdr[12..16].try_into().unwrap());

        if cmsg_len < 16 || offset + cmsg_len > control_len {
            break;
        }

        // SOL_SOCKET = 1, SCM_RIGHTS = 1
        if cmsg_level == 1 && cmsg_type == 1 {
            let fd_bytes = cmsg_len - 16;
            let num_fds = fd_bytes / 4;
            let mut raw_fds = alloc::vec![0u8; num_fds * 4];
            UserPtr::new(control_ptr + offset + 16).read_bytes(vmar, &mut raw_fds)?;
            let fd_table = proc.fd_table.lock();
            for i in 0..num_fds {
                let fd = i32::from_ne_bytes(raw_fds[i * 4..(i + 1) * 4].try_into().unwrap());
                let file = fd_table.get(fd)?;
                attached_files.push(file);
            }
        }

        let aligned_len = (cmsg_len + 7) & !7;
        if aligned_len == 0 {
            break;
        }
        offset += aligned_len;
    }

    Ok(attached_files)
}

/// Formats received file descriptors into `msghdr.msg_control` as an `SCM_RIGHTS` control message.
fn write_control_files(
    proc: &alloc::sync::Arc<crate::proc::Proc>,
    vmar: &alloc::sync::Arc<crate::vm::Vmar>,
    msghdr: &mut MsgHdr,
    files: alloc::vec::Vec<alloc::sync::Arc<crate::fs::vfs::File>>,
) -> Result<()> {
    if files.is_empty() {
        msghdr.msg_controllen = 0;
        return Ok(());
    }

    let mut new_fds = alloc::vec::Vec::with_capacity(files.len());
    {
        let mut fd_table = proc.fd_table.lock();
        for file in files {
            let new_fd = fd_table.alloc_fd(file, crate::fs::fd::FdFlags::empty())?;
            new_fds.push(new_fd);
        }
    }

    let required_len = 16 + new_fds.len() * 4;
    let user_cap = msghdr.msg_controllen;
    if msghdr.msg_control != 0 && user_cap >= 16 {
        let to_write_len = required_len.min(user_cap);
        let mut hdr = [0u8; 16];
        hdr[0..8].copy_from_slice(&to_write_len.to_ne_bytes());
        hdr[8..12].copy_from_slice(&1i32.to_ne_bytes()); // SOL_SOCKET
        hdr[12..16].copy_from_slice(&1i32.to_ne_bytes()); // SCM_RIGHTS
        UserPtr::new(msghdr.msg_control).write_bytes(vmar, &hdr)?;

        let max_fds = (user_cap - 16) / 4;
        let copy_fds = new_fds.len().min(max_fds);
        for (i, fd) in new_fds.iter().take(copy_fds).enumerate() {
            UserPtr::new(msghdr.msg_control + 16 + i * 4).write_bytes(vmar, &fd.to_ne_bytes())?;
        }

        if required_len > user_cap {
            msghdr.msg_flags |= 0x20; // MSG_CTRUNC
            msghdr.msg_controllen = to_write_len;
        } else {
            msghdr.msg_controllen = required_len;
        }
    } else {
        msghdr.msg_controllen = 0;
        msghdr.msg_flags |= 0x20; // MSG_CTRUNC
    }

    Ok(())
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
