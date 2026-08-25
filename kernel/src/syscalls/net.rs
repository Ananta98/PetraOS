//! POSIX Network and Socket System Call Handlers
//!
//! Implements Linux ABI-compatible system calls for network socket creation,
//! connection management, datagram and stream I/O, addresses, and socket options.

use alloc::sync::Arc;
use core::mem::size_of;

use crate::arch::syscall::SyscallFrame;
use crate::fs::create_socket_file;
use crate::fs::fd::FD_CLOEXEC;
use crate::fs::vfs::types::*;
use crate::net::socket::{Socket, UnixSocket};
use crate::net::types::*;
use crate::sync::spinlock::Spinlock;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};

/// Helper: Extract active `Socket` Arc from process file descriptor.
fn get_socket(fd: i32) -> Result<Arc<Spinlock<Socket>>, SyscallError> {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    file.ops.as_socket().ok_or(SyscallError::ENOTSOCK)
}

/// `sys_socket` (SYS_SOCKET = 41)
/// Create an endpoint for communication.
pub fn sys_socket(frame: &mut SyscallFrame) -> SyscallResult {
    let domain = frame.arg1() as i32;
    let socket_type = frame.arg2() as i32;
    let protocol = frame.arg3() as i32;

    let socket = Socket::new(domain, socket_type, protocol)?;
    let socket_arc = Arc::new(Spinlock::new(socket));

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

/// `sys_connect` (SYS_CONNECT = 42)
/// Initiate a connection on a socket.
pub fn sys_connect(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let addr_ptr = UserPtr::<SockAddrStorage>::from_u64(frame.arg2());
    let addrlen = frame.arg3() as usize;

    if addr_ptr.is_null() || addrlen < size_of::<u16>() {
        return Err(SyscallError::EINVAL);
    }

    let addr = addr_ptr.read().ok_or(SyscallError::EFAULT)?;
    let socket_arc = get_socket(fd)?;

    let mut sock = socket_arc.lock();
    sock.connect(&socket_arc, &addr, addrlen)?;

    Ok(0)
}

/// `sys_accept` (SYS_ACCEPT = 43)
/// Accept a connection on a socket.
pub fn sys_accept(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let addr_ptr = UserPtr::<SockAddrStorage>::from_u64(frame.arg2());
    let addrlen_ptr = UserPtr::<SockLen>::from_u64(frame.arg3());

    accept_internal(fd, addr_ptr, addrlen_ptr, 0)
}

/// `sys_accept4` (SYS_ACCEPT4 = 288)
/// Accept a connection on a socket with flags (SOCK_NONBLOCK, SOCK_CLOEXEC).
pub fn sys_accept4(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let addr_ptr = UserPtr::<SockAddrStorage>::from_u64(frame.arg2());
    let addrlen_ptr = UserPtr::<SockLen>::from_u64(frame.arg3());
    let flags = frame.arg4() as i32;

    accept_internal(fd, addr_ptr, addrlen_ptr, flags)
}

fn accept_internal(
    fd: i32,
    addr_ptr: UserPtr<SockAddrStorage>,
    addrlen_ptr: UserPtr<SockLen>,
    flags: i32,
) -> SyscallResult {
    let socket_arc = get_socket(fd)?;
    let (conn_sock, storage, actual_len) = {
        let mut sock = socket_arc.lock();
        sock.accept(flags)?
    };

    if !addr_ptr.is_null() && !addrlen_ptr.is_null() {
        let max_len = addrlen_ptr.read().ok_or(SyscallError::EFAULT)? as usize;
        let copy_len = core::cmp::min(max_len, actual_len);

        let user_slice = UserPtr::<u8>::from_u64(addr_ptr.as_u64())
            .as_slice_mut(copy_len)
            .ok_or(SyscallError::EFAULT)?;
        // SAFETY: copy_len is bounded by storage size and user buffer size.
        unsafe {
            let src = &storage as *const _ as *const u8;
            core::ptr::copy_nonoverlapping(src, user_slice.as_mut_ptr(), copy_len);
        }

        addrlen_ptr
            .write(actual_len as SockLen)
            .ok_or(SyscallError::EFAULT)?;
    }

    let file_flags = if (flags & SOCK_NONBLOCK) != 0 {
        crate::fs::vfs::types::O_NONBLOCK | crate::fs::vfs::types::O_RDWR
    } else {
        crate::fs::vfs::types::O_RDWR
    };

    let file = create_socket_file(conn_sock, file_flags);
    let desc_flags = if (flags & SOCK_CLOEXEC) != 0 {
        FD_CLOEXEC
    } else {
        0
    };

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let new_fd = proc.fd_table.alloc_with_flags(file, desc_flags);

    Ok(new_fd as usize)
}

/// `sys_sendto` (SYS_SENDTO = 44)
/// Send a message on a socket to a specific destination.
pub fn sys_sendto(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let buf_ptr = UserPtr::<u8>::from_u64(frame.arg2());
    let len = frame.arg3() as usize;
    let flags = frame.arg4() as i32;
    let dest_addr_ptr = UserPtr::<SockAddrStorage>::from_u64(frame.arg5());
    let addrlen = frame.arg6() as usize;

    let buf_slice = buf_ptr.as_slice(len).ok_or(SyscallError::EFAULT)?;
    let socket_arc = get_socket(fd)?;

    let maybe_dest = if !dest_addr_ptr.is_null() && addrlen >= size_of::<u16>() {
        Some(dest_addr_ptr.read().ok_or(SyscallError::EFAULT)?)
    } else {
        None
    };

    let mut sock = socket_arc.lock();
    let sent = sock.sendto(buf_slice, maybe_dest.as_ref(), addrlen, flags)?;
    Ok(sent)
}

/// `sys_recvfrom` (SYS_RECVFROM = 45)
/// Receive a message from a socket and capture sender address.
pub fn sys_recvfrom(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let buf_ptr = UserPtr::<u8>::from_u64(frame.arg2());
    let len = frame.arg3() as usize;
    let flags = frame.arg4() as i32;
    let src_addr_ptr = UserPtr::<SockAddrStorage>::from_u64(frame.arg5());
    let addrlen_ptr = UserPtr::<SockLen>::from_u64(frame.arg6());

    let buf_slice = buf_ptr.as_slice_mut(len).ok_or(SyscallError::EFAULT)?;
    let socket_arc = get_socket(fd)?;

    let mut sock = socket_arc.lock();
    let (received, storage, actual_len) = sock.recvfrom(buf_slice, flags)?;

    if !src_addr_ptr.is_null() && !addrlen_ptr.is_null() {
        let max_len = addrlen_ptr.read().ok_or(SyscallError::EFAULT)? as usize;
        let copy_len = core::cmp::min(max_len, actual_len);

        let user_slice = UserPtr::<u8>::from_u64(src_addr_ptr.as_u64())
            .as_slice_mut(copy_len)
            .ok_or(SyscallError::EFAULT)?;
        // SAFETY: copy_len is bounded by storage size and user buffer size.
        unsafe {
            let src = &storage as *const _ as *const u8;
            core::ptr::copy_nonoverlapping(src, user_slice.as_mut_ptr(), copy_len);
        }

        addrlen_ptr
            .write(actual_len as SockLen)
            .ok_or(SyscallError::EFAULT)?;
    }

    Ok(received)
}

/// `sys_sendmsg` (SYS_SENDMSG = 46)
/// Send a message on a socket using a message header.
pub fn sys_sendmsg(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let msg_ptr = UserPtr::<MsgHdr>::from_u64(frame.arg2());
    let flags = frame.arg3() as i32;

    let msg = msg_ptr.read().ok_or(SyscallError::EFAULT)?;
    let iov_slice = UserPtr::<IoVec>::from_u64(msg.msg_iov)
        .as_slice(msg.msg_iovlen)
        .ok_or(SyscallError::EFAULT)?;

    let dest_storage = if msg.msg_name != 0 && msg.msg_namelen >= size_of::<u16>() as u32 {
        Some(
            UserPtr::<SockAddrStorage>::from_u64(msg.msg_name)
                .read()
                .ok_or(SyscallError::EFAULT)?,
        )
    } else {
        None
    };

    let socket_arc = get_socket(fd)?;
    let mut total_sent = 0;

    for iov in iov_slice {
        if iov.iov_len == 0 {
            continue;
        }
        let chunk = UserPtr::<u8>::from_u64(iov.iov_base)
            .as_slice(iov.iov_len)
            .ok_or(SyscallError::EFAULT)?;

        let mut sock = socket_arc.lock();
        let sent = sock.sendto(
            chunk,
            dest_storage.as_ref(),
            msg.msg_namelen as usize,
            flags,
        )?;
        total_sent += sent;
        if sent < iov.iov_len {
            break;
        }
    }

    Ok(total_sent)
}

/// `sys_recvmsg` (SYS_RECVMSG = 47)
/// Receive a message from a socket into multiple scatter-gather buffers.
pub fn sys_recvmsg(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let msg_ptr = UserPtr::<MsgHdr>::from_u64(frame.arg2());
    let flags = frame.arg3() as i32;

    let mut msg = msg_ptr.read().ok_or(SyscallError::EFAULT)?;
    let iov_slice = UserPtr::<IoVec>::from_u64(msg.msg_iov)
        .as_slice(msg.msg_iovlen)
        .ok_or(SyscallError::EFAULT)?;

    let socket_arc = get_socket(fd)?;
    let mut total_received = 0;
    let mut last_addr: Option<(SockAddrStorage, usize)> = None;

    for iov in iov_slice {
        if iov.iov_len == 0 {
            continue;
        }
        let chunk = UserPtr::<u8>::from_u64(iov.iov_base)
            .as_slice_mut(iov.iov_len)
            .ok_or(SyscallError::EFAULT)?;

        let mut sock = socket_arc.lock();
        let (n, storage, addr_len) = sock.recvfrom(chunk, flags)?;
        last_addr = Some((storage, addr_len));
        total_received += n;
        if n < iov.iov_len {
            break;
        }
    }

    if let Some((storage, actual_len)) = last_addr {
        if msg.msg_name != 0 && msg.msg_namelen > 0 {
            let copy_len = core::cmp::min(msg.msg_namelen as usize, actual_len);
            let user_slice = UserPtr::<u8>::from_u64(msg.msg_name)
                .as_slice_mut(copy_len)
                .ok_or(SyscallError::EFAULT)?;
            // SAFETY: copy_len is safely bounded.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &storage as *const _ as *const u8,
                    user_slice.as_mut_ptr(),
                    copy_len,
                );
            }
            msg.msg_namelen = actual_len as u32;
            msg_ptr.write(msg).ok_or(SyscallError::EFAULT)?;
        }
    }

    Ok(total_received)
}

/// `sys_shutdown` (SYS_SHUTDOWN = 48)
/// Shut down part of a full-duplex connection.
pub fn sys_shutdown(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let how = frame.arg2() as i32;

    let socket_arc = get_socket(fd)?;
    socket_arc.lock().shutdown(how)?;

    Ok(0)
}

/// `sys_bind` (SYS_BIND = 49)
/// Bind a name to a socket.
pub fn sys_bind(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let addr_ptr = UserPtr::<SockAddrStorage>::from_u64(frame.arg2());
    let addrlen = frame.arg3() as usize;

    if addr_ptr.is_null() || addrlen < size_of::<u16>() {
        return Err(SyscallError::EINVAL);
    }

    let addr = addr_ptr.read().ok_or(SyscallError::EFAULT)?;
    let socket_arc = get_socket(fd)?;

    let mut sock = socket_arc.lock();
    sock.bind(&socket_arc, &addr, addrlen)?;

    Ok(0)
}

/// `sys_listen` (SYS_LISTEN = 50)
/// Listen for connections on a socket.
pub fn sys_listen(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let backlog = frame.arg2() as usize;

    let socket_arc = get_socket(fd)?;
    socket_arc.lock().listen(backlog)?;

    Ok(0)
}

/// `sys_getsockname` (SYS_GETSOCKNAME = 51)
/// Get local socket name / address.
pub fn sys_getsockname(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let addr_ptr = UserPtr::<SockAddrStorage>::from_u64(frame.arg2());
    let addrlen_ptr = UserPtr::<SockLen>::from_u64(frame.arg3());

    if addr_ptr.is_null() || addrlen_ptr.is_null() {
        return Err(SyscallError::EFAULT);
    }

    let socket_arc = get_socket(fd)?;
    let (_ep, storage, actual_len) = socket_arc.lock().getsockname()?;

    let max_len = addrlen_ptr.read().ok_or(SyscallError::EFAULT)? as usize;
    let copy_len = core::cmp::min(max_len, actual_len);

    let user_slice = UserPtr::<u8>::from_u64(addr_ptr.as_u64())
        .as_slice_mut(copy_len)
        .ok_or(SyscallError::EFAULT)?;
    // SAFETY: copy_len is bounded.
    unsafe {
        core::ptr::copy_nonoverlapping(
            &storage as *const _ as *const u8,
            user_slice.as_mut_ptr(),
            copy_len,
        );
    }

    addrlen_ptr
        .write(actual_len as SockLen)
        .ok_or(SyscallError::EFAULT)?;

    Ok(0)
}

/// `sys_getpeername` (SYS_GETPEERNAME = 52)
/// Get remote peer name / address.
pub fn sys_getpeername(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let addr_ptr = UserPtr::<SockAddrStorage>::from_u64(frame.arg2());
    let addrlen_ptr = UserPtr::<SockLen>::from_u64(frame.arg3());

    if addr_ptr.is_null() || addrlen_ptr.is_null() {
        return Err(SyscallError::EFAULT);
    }

    let socket_arc = get_socket(fd)?;
    let (_ep, storage, actual_len) = socket_arc.lock().getpeername()?;

    let max_len = addrlen_ptr.read().ok_or(SyscallError::EFAULT)? as usize;
    let copy_len = core::cmp::min(max_len, actual_len);

    let user_slice = UserPtr::<u8>::from_u64(addr_ptr.as_u64())
        .as_slice_mut(copy_len)
        .ok_or(SyscallError::EFAULT)?;
    // SAFETY: copy_len is bounded.
    unsafe {
        core::ptr::copy_nonoverlapping(
            &storage as *const _ as *const u8,
            user_slice.as_mut_ptr(),
            copy_len,
        );
    }

    addrlen_ptr
        .write(actual_len as SockLen)
        .ok_or(SyscallError::EFAULT)?;

    Ok(0)
}

/// `sys_socketpair` (SYS_SOCKETPAIR = 53)
/// Create a pair of connected sockets.
pub fn sys_socketpair(frame: &mut SyscallFrame) -> SyscallResult {
    let domain = frame.arg1() as i32;
    let socket_type = frame.arg2() as i32;
    let _protocol = frame.arg3() as i32;
    let sv_ptr = UserPtr::<[i32; 2]>::from_u64(frame.arg4());

    if domain as u16 != AF_UNIX {
        return Err(SyscallError::EAFNOSUPPORT);
    }

    let actual_type = socket_type & SOCK_TYPE_MASK;
    let nonblocking = (socket_type & SOCK_NONBLOCK) != 0;
    let cloexec = (socket_type & SOCK_CLOEXEC) != 0;

    let (sock_a, sock_b) = UnixSocket::create_pair(actual_type, nonblocking);

    let file_flags = if nonblocking {
        O_NONBLOCK | O_RDWR
    } else {
        O_RDWR
    };

    let desc_flags = if cloexec { FD_CLOEXEC } else { 0 };

    let file_a = create_socket_file(Arc::new(Spinlock::new(Socket::Unix(sock_a))), file_flags);
    let file_b = create_socket_file(Arc::new(Spinlock::new(Socket::Unix(sock_b))), file_flags);

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let fd_a = proc.fd_table.alloc_with_flags(file_a, desc_flags);
    let fd_b = proc.fd_table.alloc_with_flags(file_b, desc_flags);

    sv_ptr.write([fd_a, fd_b]).ok_or(SyscallError::EFAULT)?;

    Ok(0)
}

/// `sys_setsockopt` (SYS_SETSOCKOPT = 54)
/// Set options on sockets.
pub fn sys_setsockopt(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let _level = frame.arg2() as i32;
    let optname = frame.arg3() as i32;
    let optval_ptr = UserPtr::<i32>::from_u64(frame.arg4());
    let _optlen = frame.arg5() as usize;

    let _socket_arc = get_socket(fd)?;

    if !optval_ptr.is_null() {
        let val = optval_ptr.read().ok_or(SyscallError::EFAULT)?;
        match optname {
            SO_REUSEADDR | SO_REUSEPORT | SO_KEEPALIVE | TCP_NODELAY => {
                log::trace!("[setsockopt] optname {} set to {}", optname, val);
            }
            SO_RCVTIMEO | SO_SNDTIMEO => {
                log::trace!("[setsockopt] timeout optname {} configured", optname);
            }
            _ => {}
        }
    }

    Ok(0)
}

/// `sys_getsockopt` (SYS_GETSOCKOPT = 55)
/// Get options on sockets.
pub fn sys_getsockopt(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let _level = frame.arg2() as i32;
    let optname = frame.arg3() as i32;
    let optval_ptr = UserPtr::<i32>::from_u64(frame.arg4());
    let optlen_ptr = UserPtr::<SockLen>::from_u64(frame.arg5());

    if optval_ptr.is_null() || optlen_ptr.is_null() {
        return Err(SyscallError::EFAULT);
    }

    let socket_arc = get_socket(fd)?;

    match optname {
        SO_TYPE => {
            let sock_type = match &*socket_arc.lock() {
                Socket::Tcp(_) => SOCK_STREAM,
                Socket::Udp(_) => SOCK_DGRAM,
                Socket::Raw(_) => SOCK_RAW,
                Socket::Unix(u) => u.lock().socket_type,
            };
            optval_ptr.write(sock_type).ok_or(SyscallError::EFAULT)?;
            optlen_ptr
                .write(size_of::<i32>() as SockLen)
                .ok_or(SyscallError::EFAULT)?;
        }
        SO_ERROR => {
            optval_ptr.write(0).ok_or(SyscallError::EFAULT)?;
            optlen_ptr
                .write(size_of::<i32>() as SockLen)
                .ok_or(SyscallError::EFAULT)?;
        }
        _ => {
            optval_ptr.write(0).ok_or(SyscallError::EFAULT)?;
            optlen_ptr
                .write(size_of::<i32>() as SockLen)
                .ok_or(SyscallError::EFAULT)?;
        }
    }

    Ok(0)
}
