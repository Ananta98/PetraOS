//! sys_getsockname system call handler.

use super::*;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_getsockname` (SYS_GETSOCKNAME = 51)
/// Get local socket name / address.
#[wrap_syscall]
pub fn sys_getsockname(
    fd: i32,
    addr_ptr: UserPtr<SockAddrStorage>,
    addrlen_ptr: UserPtr<SockLen>,
) -> SyscallResult {
    if addr_ptr.is_null() || addrlen_ptr.is_null() {
        return Err(SyscallError::EFAULT);
    }

    let socket_arc = get_socket(fd)?;
    let (_ep, storage, actual_len) = socket_arc.lock().getsockname()?;

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
