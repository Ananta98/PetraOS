// SPDX-License-Identifier: GPL-2.0

//! `rt_sigtimedwait(2)` system call (Syscall #128 on x86_64).

use ostd::arch::cpu::context::UserContext;

use super::utils::decode_sigset;
use crate::{
    arch::signal::encode_siginfo,
    proc::thread::{Thread, ThreadFlags},
    time::Timespec,
    uapi::{
        errno::{Errno, Result},
        signal::SIGSET_WORDS,
    },
    utils::UserPtr,
};

/// `rt_sigtimedwait(2)` - synchronously wait for queued signals.
pub fn sys_rt_sigtimedwait(
    set_ptr: usize,
    info_ptr: usize,
    timeout_ptr: usize,
    sigsetsize: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if sigsetsize != 8 && sigsetsize != SIGSET_WORDS * 8 {
        return Err(Errno::EINVAL);
    }
    if set_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let td = Thread::current().ok_or(Errno::ESRCH)?;
    let proc = td.proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let mut buf = [0u8; SIGSET_WORDS * 8];
    UserPtr::new(set_ptr).read_bytes(&vmar, &mut buf[..sigsetsize])?;
    let requested = decode_sigset(&buf);

    let timeout_ns = if timeout_ptr != 0 {
        let mut tbuf = [0u8; 16];
        UserPtr::new(timeout_ptr).read_bytes(&vmar, &mut tbuf)?;
        let ts = Timespec::from_bytes(&tbuf).ok_or(Errno::EINVAL)?;
        if !ts.is_valid() {
            return Err(Errno::EINVAL);
        }
        Some(ts.total_nanos().ok_or(Errno::EINVAL)?)
    } else {
        None
    };

    let start_ns = crate::time::get_monotonic_ns();

    loop {
        // Check thread queue then process queue for matching signal
        let queued = td
            .inner
            .lock()
            .sigqueue
            .take_matching(|sig| requested.contains(sig))
            .or_else(|| proc.sigqueue.take_matching(|sig| requested.contains(sig)));

        if let Some(q) = queued {
            if info_ptr != 0 {
                let bytes = encode_siginfo(&q.info);
                UserPtr::new(info_ptr).write_bytes(&vmar, &bytes)?;
            }
            return Ok(q.info.si_signo as usize);
        }

        if let Some(tns) = timeout_ns {
            let elapsed = crate::time::get_monotonic_ns().saturating_sub(start_ns);
            if elapsed >= tns {
                return Err(Errno::EAGAIN);
            }
        }

        // Check if an unblocked signal not in the requested wait set arrived
        let inner = td.inner.lock();
        if inner.flags.contains(ThreadFlags::TDF_ASTPENDING) {
            let has_unmasked = inner.sigqueue.has_unmasked(&inner.sigmask)
                || proc.sigqueue.has_unmasked(&inner.sigmask);
            if has_unmasked {
                drop(inner);
                return Err(Errno::EINTR);
            }
        }
        drop(inner);

        ostd::task::Task::yield_now();
    }
}
