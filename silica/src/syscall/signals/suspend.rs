// SPDX-License-Identifier: GPL-2.0

//! `rt_sigsuspend(2)` system call (Syscall #130 on x86_64).

use ostd::{arch::cpu::context::UserContext, task::Task};

use super::utils::decode_sigset;
use crate::{
    api::{
        errno::{Errno, Result},
        signal::SIGSET_WORDS,
    },
    proc::thread::{Thread, ThreadFlags},
    utils::userspace::user_ptr::UserPtr,
};

/// `rt_sigsuspend(2)`.
pub fn sys_rt_sigsuspend(
    unewset_ptr: usize,
    sigsetsize: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if sigsetsize != 8 && sigsetsize != SIGSET_WORDS * 8 {
        return Err(Errno::EINVAL);
    }
    if unewset_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let td = Thread::current().ok_or(Errno::ESRCH)?;
    let proc = td.proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let mut buf = [0u8; SIGSET_WORDS * 8];
    UserPtr::new(unewset_ptr).read_bytes(&vmar, &mut buf[..sigsetsize])?;
    let mut requested = decode_sigset(&buf);
    requested.clear_unblockable();

    let old_mask = {
        let mut inner = td.inner.lock();
        let old = inner.sigmask;
        inner.sigmask = requested;
        inner.flags.insert(ThreadFlags::TDF_ASTPENDING);
        old
    };

    loop {
        if td.inner.lock().flags.contains(ThreadFlags::TDF_ASTPENDING) {
            let inner = td.inner.lock();
            let thread_has = inner.sigqueue.has_unmasked(&inner.sigmask);
            let proc_has = proc.sigqueue.has_unmasked(&inner.sigmask);
            if thread_has || proc_has {
                drop(inner);
                td.inner.lock().sigmask = old_mask;
                return Err(Errno::EINTR);
            }
        }
        Task::yield_now();
    }
}
