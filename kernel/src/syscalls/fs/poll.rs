//! System calls for synchronous I/O multiplexing (`poll`, `ppoll`, `select`, `pselect6`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use alloc::sync::Arc;
use alloc::vec::Vec;

pub const POLLIN: i16 = 0x0001;
pub const POLLPRI: i16 = 0x0002;
pub const POLLOUT: i16 = 0x0004;
pub const POLLERR: i16 = 0x0008;
pub const POLLHUP: i16 = 0x0010;
pub const POLLNVAL: i16 = 0x0020;

pub const FD_SETSIZE: usize = 1024;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FdSet {
    pub fds_bits: [u64; FD_SETSIZE / 64],
}

pub(crate) fn do_poll(fds_ptr: UserPtr<PollFd>, nfds: usize, timeout_ms: i32) -> SyscallResult {
    if nfds == 0 {
        if timeout_ms > 0 {
            let start_ns = crate::arch::timer::hpet::elapsed_ns();
            let dur_ns = (timeout_ms as u64) * 1_000_000;
            while crate::arch::timer::hpet::elapsed_ns().saturating_sub(start_ns) < dur_ns {
                crate::arch::enable_interrupts();
                crate::proc::thread::Thread::yield_cpu();
            }
        }
        return Ok(0);
    }
    if nfds > 1024 {
        return Err(SyscallError::EFAULT);
    }
    let fds_slice = fds_ptr.as_slice_mut(nfds).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;

    let start_ns = crate::arch::timer::hpet::elapsed_ns();
    let has_timeout = timeout_ms >= 0;
    let dur_ns = if timeout_ms > 0 {
        (timeout_ms as u64) * 1_000_000
    } else {
        0
    };

    loop {
        // Retrieve file handles under proc lock, then drop proc lock before polling events
        let files: Vec<Option<Arc<File>>> = {
            let proc = proc_arc.lock();
            fds_slice
                .iter()
                .map(|pfd| {
                    if pfd.fd >= 0 {
                        proc.fd_table.get(pfd.fd).ok()
                    } else {
                        None
                    }
                })
                .collect()
        };

        let mut ready_count = 0;
        for (pfd, file_opt) in fds_slice.iter_mut().zip(files.iter()) {
            if pfd.fd < 0 {
                pfd.revents = 0;
                continue;
            }
            match file_opt {
                Some(file) => {
                    let revents = file.ops.poll_events(pfd.events);
                    pfd.revents = revents;
                    if revents != 0 {
                        ready_count += 1;
                    }
                }
                None => {
                    pfd.revents = POLLNVAL;
                    ready_count += 1;
                }
            }
        }

        if ready_count > 0 {
            return Ok(ready_count);
        }

        if has_timeout {
            if timeout_ms == 0 {
                return Ok(0);
            }
            if crate::arch::timer::hpet::elapsed_ns().saturating_sub(start_ns) >= dur_ns {
                return Ok(0);
            }
        }

        // Check if there are pending signals interrupting poll
        {
            let proc = proc_arc.lock();
            if proc.pending_signals.mask != 0 {
                return Err(SyscallError::EINTR);
            }
        }

        crate::arch::enable_and_hlt();
    }
}

/// `sys_poll` (SYS_POLL = 7)
/// Wait for file descriptors to become ready for I/O.
pub fn sys_poll(frame: &mut SyscallFrame) -> SyscallResult {
    let fds_ptr = UserPtr::<PollFd>::from_u64(frame.arg1());
    let nfds = frame.arg2() as usize;
    let timeout_ms = frame.arg3() as i32;

    do_poll(fds_ptr, nfds, timeout_ms)
}

/// `sys_ppoll` (SYS_PPOLL = 271)
/// Wait for file descriptors with a timespec timeout.
pub fn sys_ppoll(frame: &mut SyscallFrame) -> SyscallResult {
    let fds_ptr = UserPtr::<PollFd>::from_u64(frame.arg1());
    let nfds = frame.arg2() as usize;
    let ts_ptr = UserPtr::<crate::syscalls::time::TimeSpec>::from_u64(frame.arg3());

    let timeout_ms = if ts_ptr.is_null() {
        -1
    } else {
        let ts = ts_ptr.read().ok_or(SyscallError::EFAULT)?;
        if ts.tv_sec < 0 || ts.tv_nsec < 0 {
            return Err(SyscallError::EINVAL);
        }
        (ts.tv_sec * 1000 + ts.tv_nsec / 1_000_000) as i32
    };

    do_poll(fds_ptr, nfds, timeout_ms)
}

/// `sys_select` (SYS_SELECT = 23)
/// Synchronous I/O multiplexing with file descriptor sets.
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

/// `sys_pselect6` (SYS_PSELECT6 = 270)
/// Synchronous I/O multiplexing with signal mask.
pub fn sys_pselect6(frame: &mut SyscallFrame) -> SyscallResult {
    sys_select(frame)
}
