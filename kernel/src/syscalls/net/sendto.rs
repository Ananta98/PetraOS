//! sys_sendto system call handler.

use super::*;
use alloc::sync::Arc;
use core::mem::size_of;
use crate::arch::syscall::SyscallFrame;
use crate::fs::create_socket_file;
use crate::fs::fd::FD_CLOEXEC;
use crate::fs::vfs::types::*;
use crate::net::socket::{Socket, UnixSocket};
use crate::net::types::*;
use crate::sync::Mutex;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};


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
