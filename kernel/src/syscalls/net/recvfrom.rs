//! sys_recvfrom system call handler.

use super::*;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_recvfrom` (SYS_RECVFROM = 45)
/// Receive a message from a socket and capture sender address.
#[wrap_syscall]
pub fn sys_recvfrom(
    fd: i32,
    buf_ptr: UserPtr<u8>,
    len: usize,
    flags: i32,
    src_addr_ptr: UserPtr<SockAddrStorage>,
    addrlen_ptr: UserPtr<SockLen>,
) -> SyscallResult {
    let buf_slice = buf_ptr.as_slice_mut(len).ok_or(SyscallError::EFAULT)?;
    let socket_arc = get_socket(fd)?;

    let mut sock = socket_arc.lock();
    let (received, storage, actual_len) = sock.recvfrom(buf_slice, flags)?;

    if !src_addr_ptr.is_null() && !addrlen_ptr.is_null() {
        let max_len = addrlen_ptr.read().ok_or(SyscallError::EFAULT)? as usize;
        let copy_len = core::cmp::min(max_len, actual_len);

        let user_slice = UserPtr::<u8>::from_u64(src_addr_ptr.as_u64())
            .as_slice_mut(copy_len)
            .ok_or(SyscallError::EFAULT)?;
        // SAFETY: copy_len is bounded by storage size and user buffer size.
        unsafe {
            let src = &storage as *const _ as *const u8;
            core::ptr::copy_nonoverlapping(src, user_slice.as_mut_ptr(), copy_len);
        }

        addrlen_ptr
            .write(actual_len as SockLen)
            .ok_or(SyscallError::EFAULT)?;
    }

    Ok(received)
}
