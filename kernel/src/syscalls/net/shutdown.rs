//! sys_shutdown system call handler.

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


/// `sys_shutdown` (SYS_SHUTDOWN = 48)
/// Shut down part of a full-duplex connection.
pub fn sys_shutdown(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let how = frame.arg2() as i32;

    let socket_arc = get_socket(fd)?;
    socket_arc.lock().shutdown(how)?;

    Ok(0)
}
