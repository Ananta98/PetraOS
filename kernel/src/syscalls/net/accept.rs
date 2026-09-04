//! sys_accept system call handler.

use super::*;
use crate::arch::syscall::SyscallFrame;
use crate::syscalls::{SyscallResult, UserPtr};


/// `sys_accept` (SYS_ACCEPT = 43)
/// Accept a connection on a socket.
pub fn sys_accept(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let addr_ptr = UserPtr::<SockAddrStorage>::from_u64(frame.arg2());
    let addrlen_ptr = UserPtr::<SockLen>::from_u64(frame.arg3());

    accept_internal(fd, addr_ptr, addrlen_ptr, 0)
}
