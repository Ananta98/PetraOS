//! sys_accept4 system call handler.

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


/// `sys_accept4` (SYS_ACCEPT4 = 288)
/// Accept a connection on a socket with flags (SOCK_NONBLOCK, SOCK_CLOEXEC).
pub fn sys_accept4(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let addr_ptr = UserPtr::<SockAddrStorage>::from_u64(frame.arg2());
    let addrlen_ptr = UserPtr::<SockLen>::from_u64(frame.arg3());
    let flags = frame.arg4() as i32;

    accept_internal(fd, addr_ptr, addrlen_ptr, flags)
}
