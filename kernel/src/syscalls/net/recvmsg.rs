//! sys_recvmsg system call handler.

use super::*;
use crate::arch::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};


/// `sys_recvmsg` (SYS_RECVMSG = 47)
/// Receive a message from a socket into multiple scatter-gather buffers.
pub fn sys_recvmsg(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let msg_ptr = UserPtr::<MsgHdr>::from_u64(frame.arg2());
    let flags = frame.arg3() as i32;

    let mut msg = msg_ptr.read().ok_or(SyscallError::EFAULT)?;
    let iov_slice = UserPtr::<IoVec>::from_u64(msg.msg_iov)
        .as_slice(msg.msg_iovlen)
        .ok_or(SyscallError::EFAULT)?;

    let socket_arc = get_socket(fd)?;
    let mut total_received = 0;
    let mut last_addr: Option<(SockAddrStorage, usize)> = None;

    for iov in iov_slice {
        if iov.iov_len == 0 {
            continue;
        }
        let chunk = UserPtr::<u8>::from_u64(iov.iov_base)
            .as_slice_mut(iov.iov_len)
            .ok_or(SyscallError::EFAULT)?;

        let mut sock = socket_arc.lock();
        let (n, storage, addr_len) = sock.recvfrom(chunk, flags)?;
        last_addr = Some((storage, addr_len));
        total_received += n;
        if n < iov.iov_len {
            break;
        }
    }

    if let Some((storage, actual_len)) = last_addr {
        if msg.msg_name != 0 && msg.msg_namelen > 0 {
            let copy_len = core::cmp::min(msg.msg_namelen as usize, actual_len);
            let user_slice = UserPtr::<u8>::from_u64(msg.msg_name)
                .as_slice_mut(copy_len)
                .ok_or(SyscallError::EFAULT)?;
            // SAFETY: copy_len is safely bounded.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &storage as *const _ as *const u8,
                    user_slice.as_mut_ptr(),
                    copy_len,
                );
            }
            msg.msg_namelen = actual_len as u32;
            msg_ptr.write(msg).ok_or(SyscallError::EFAULT)?;
        }
    }

    Ok(total_received)
}
