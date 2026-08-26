//! sys_select system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;


/// `sys_select` (SYS_SELECT = 23)
pub fn sys_select(frame: &mut SyscallFrame) -> SyscallResult {
    let nfds = frame.arg1() as i32;
    let readfds = UserPtr::<FdSet>::from_u64(frame.arg2());
    let writefds = UserPtr::<FdSet>::from_u64(frame.arg3());
    let exceptfds = UserPtr::<FdSet>::from_u64(frame.arg4());
    let _timeout = UserPtr::<LinuxTimespec>::from_u64(frame.arg5());

    if nfds < 0 || nfds > FD_SETSIZE as i32 {
        return Err(SyscallError::EINVAL);
    }
    if nfds == 0 {
        return Ok(0);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let mut ready_count = 0;
    let mut rfds_val = if !readfds.is_null() {
        Some(readfds.read().ok_or(SyscallError::EFAULT)?)
    } else {
        None
    };
    let mut wfds_val = if !writefds.is_null() {
        Some(writefds.read().ok_or(SyscallError::EFAULT)?)
    } else {
        None
    };
    let mut efds_val = if !exceptfds.is_null() {
        Some(exceptfds.read().ok_or(SyscallError::EFAULT)?)
    } else {
        None
    };

    for fd in 0..nfds {
        let word = (fd / 64) as usize;
        let bit = 1u64 << (fd % 64);

        let mut is_r = false;
        let mut is_w = false;

        if let Some(ref r) = rfds_val {
            if (r.fds_bits[word] & bit) != 0 {
                is_r = true;
            }
        }
        if let Some(ref w) = wfds_val {
            if (w.fds_bits[word] & bit) != 0 {
                is_w = true;
            }
        }
        if let Some(ref mut e) = efds_val {
            e.fds_bits[word] &= !bit;
        }

        if is_r || is_w {
            if let Ok(file) = proc.fd_table.get(fd) {
                let flags = file.flags();
                if is_r && crate::fs::can_read(flags) {
                    ready_count += 1;
                } else if let Some(ref mut r) = rfds_val {
                    r.fds_bits[word] &= !bit;
                }
                if is_w && crate::fs::can_write(flags) {
                    ready_count += 1;
                } else if let Some(ref mut w) = wfds_val {
                    w.fds_bits[word] &= !bit;
                }
            } else {
                if let Some(ref mut r) = rfds_val {
                    r.fds_bits[word] &= !bit;
                }
                if let Some(ref mut w) = wfds_val {
                    w.fds_bits[word] &= !bit;
                }
            }
        }
    }

    if let Some(r) = rfds_val {
        readfds.write(r).ok_or(SyscallError::EFAULT)?;
    }
    if let Some(w) = wfds_val {
        writefds.write(w).ok_or(SyscallError::EFAULT)?;
    }
    if let Some(e) = efds_val {
        exceptfds.write(e).ok_or(SyscallError::EFAULT)?;
    }

    Ok(ready_count)
}
