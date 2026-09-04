//! sys_bind system call handler.

use super::*;
use core::mem::size_of;
use crate::arch::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};


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
