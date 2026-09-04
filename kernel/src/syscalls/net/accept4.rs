//! sys_accept4 system call handler.

use super::*;
use crate::arch::syscall::SyscallFrame;
use crate::syscalls::{SyscallResult, UserPtr};


/// `sys_accept4` (SYS_ACCEPT4 = 288)
/// Accept a connection on a socket with flags (SOCK_NONBLOCK, SOCK_CLOEXEC).
pub fn sys_accept4(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let addr_ptr = UserPtr::<SockAddrStorage>::from_u64(frame.arg2());
    let addrlen_ptr = UserPtr::<SockLen>::from_u64(frame.arg3());
    let flags = frame.arg4() as i32;

    accept_internal(fd, addr_ptr, addrlen_ptr, flags)
}
