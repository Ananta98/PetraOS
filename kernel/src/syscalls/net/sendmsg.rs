//! sys_sendmsg system call handler.

use super::*;
use core::mem::size_of;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_sendmsg` (SYS_SENDMSG = 46)
/// Send a message on a socket using a message header.
#[wrap_syscall]
pub fn sys_sendmsg(fd: i32, msg_ptr: UserPtr<MsgHdr>, flags: i32) -> SyscallResult {
    let msg = msg_ptr.read().ok_or(SyscallError::EFAULT)?;
    let iov_slice = UserPtr::<IoVec>::from_u64(msg.msg_iov)
        .as_slice(msg.msg_iovlen)
        .ok_or(SyscallError::EFAULT)?;

    let dest_storage = if msg.msg_name != 0 && msg.msg_namelen >= size_of::<u16>() as u32 {
        Some(
            UserPtr::<SockAddrStorage>::from_u64(msg.msg_name)
                .read()
                .ok_or(SyscallError::EFAULT)?,
        )
    } else {
        None
    };

    let socket_arc = get_socket(fd)?;
    let mut total_sent = 0;

    for iov in iov_slice {
        if iov.iov_len == 0 {
            continue;
        }
        let chunk = UserPtr::<u8>::from_u64(iov.iov_base)
            .as_slice(iov.iov_len)
            .ok_or(SyscallError::EFAULT)?;

        let mut sock = socket_arc.lock();
        let sent = sock.sendto(
            chunk,
            dest_storage.as_ref(),
            msg.msg_namelen as usize,
            flags,
        )?;
        total_sent += sent;
        if sent < iov.iov_len {
            break;
        }
    }

    Ok(total_sent)
}
