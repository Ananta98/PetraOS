//! sys_listen system call handler.

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


/// `sys_listen` (SYS_LISTEN = 50)
/// Listen for connections on a socket.
pub fn sys_listen(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let backlog = frame.arg2() as usize;

    let socket_arc = get_socket(fd)?;
    socket_arc.lock().listen(backlog)?;

    Ok(0)
}
