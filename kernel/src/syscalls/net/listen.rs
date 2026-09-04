//! sys_listen system call handler.

use super::*;
use crate::arch::syscall::SyscallFrame;
use crate::syscalls::SyscallResult;


/// `sys_listen` (SYS_LISTEN = 50)
/// Listen for connections on a socket.
pub fn sys_listen(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let backlog = frame.arg2() as usize;

    let socket_arc = get_socket(fd)?;
    socket_arc.lock().listen(backlog)?;

    Ok(0)
}
