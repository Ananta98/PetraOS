//! sys_setsockopt system call handler.

use super::*;
use crate::arch::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};


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
