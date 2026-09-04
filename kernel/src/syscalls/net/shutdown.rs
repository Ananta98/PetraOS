//! sys_shutdown system call handler.

use super::*;
use crate::arch::syscall::SyscallFrame;
use crate::syscalls::SyscallResult;


/// `sys_shutdown` (SYS_SHUTDOWN = 48)
/// Shut down part of a full-duplex connection.
pub fn sys_shutdown(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let how = frame.arg2() as i32;

    let socket_arc = get_socket(fd)?;
    socket_arc.lock().shutdown(how)?;

    Ok(0)
}
