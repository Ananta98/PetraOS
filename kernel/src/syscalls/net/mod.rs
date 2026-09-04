//! POSIX Network and Socket System Call Handlers
//!
//! Implements Linux ABI-compatible system calls for network socket creation,
//! connection management, datagram and stream I/O, addresses, and socket options.

use alloc::sync::Arc;

use crate::fs::create_socket_file;
use crate::fs::fd::FD_CLOEXEC;
use crate::fs::vfs::types::*;
use crate::net::socket::Socket;
use crate::net::types::*;
use crate::sync::Mutex;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};

// ── Modular syscall submodules ──────────────────────────────────────────
pub mod socket;
pub mod connect;
pub mod accept;
pub mod accept4;
pub mod sendto;
pub mod recvfrom;
pub mod sendmsg;
pub mod recvmsg;
pub mod shutdown;
pub mod bind;
pub mod listen;
pub mod getsockname;
pub mod getpeername;
pub mod socketpair;
pub mod setsockopt;
pub mod getsockopt;

pub use socket::sys_socket;
pub use connect::sys_connect;
pub use accept::sys_accept;
pub use accept4::sys_accept4;
pub use sendto::sys_sendto;
pub use recvfrom::sys_recvfrom;
pub use sendmsg::sys_sendmsg;
pub use recvmsg::sys_recvmsg;
pub use shutdown::sys_shutdown;
pub use bind::sys_bind;
pub use listen::sys_listen;
pub use getsockname::sys_getsockname;
pub use getpeername::sys_getpeername;
pub use socketpair::sys_socketpair;
pub use setsockopt::sys_setsockopt;
pub use getsockopt::sys_getsockopt;


/// Helper: Extract active `Socket` Arc from process file descriptor.
pub(crate) fn get_socket(fd: i32) -> Result<Arc<Mutex<Socket>>, SyscallError> {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    file.ops.as_socket().ok_or(SyscallError::ENOTSOCK)
}

pub(crate) fn accept_internal(
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
