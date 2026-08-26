//! sys_getsockopt system call handler.

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
