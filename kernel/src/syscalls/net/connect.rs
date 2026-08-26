//! sys_connect system call handler.

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
