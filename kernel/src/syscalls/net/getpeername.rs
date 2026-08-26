//! sys_getpeername system call handler.

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


/// `sys_getpeername` (SYS_GETPEERNAME = 52)
/// Get remote peer name / address.
pub fn sys_getpeername(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let addr_ptr = UserPtr::<SockAddrStorage>::from_u64(frame.arg2());
    let addrlen_ptr = UserPtr::<SockLen>::from_u64(frame.arg3());

    if addr_ptr.is_null() || addrlen_ptr.is_null() {
        return Err(SyscallError::EFAULT);
    }

    let socket_arc = get_socket(fd)?;
    let (_ep, storage, actual_len) = socket_arc.lock().getpeername()?;

    let max_len = addrlen_ptr.read().ok_or(SyscallError::EFAULT)? as usize;
    let copy_len = core::cmp::min(max_len, actual_len);

    let user_slice = UserPtr::<u8>::from_u64(addr_ptr.as_u64())
        .as_slice_mut(copy_len)
        .ok_or(SyscallError::EFAULT)?;
    // SAFETY: copy_len is bounded.
    unsafe {
        core::ptr::copy_nonoverlapping(
            &storage as *const _ as *const u8,
            user_slice.as_mut_ptr(),
            copy_len,
        );
    }

    addrlen_ptr
        .write(actual_len as SockLen)
        .ok_or(SyscallError::EFAULT)?;

    Ok(0)
}
