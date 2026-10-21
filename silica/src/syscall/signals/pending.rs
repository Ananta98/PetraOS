// SPDX-License-Identifier: GPL-2.0

//! `rt_sigpending(2)` system call (Syscall #127 on x86_64).

use ostd::arch::cpu::context::UserContext;

use super::utils::encode_sigset;
use crate::{
    api::{
        errno::{Errno, Result},
        signal::SIGSET_WORDS,
    },
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
};

/// `rt_sigpending(2)`.
pub fn sys_rt_sigpending(
    set_ptr: usize,
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

    let mut pending = td.inner.lock().sigqueue.pending_set();
    let proc_pending = proc.sigqueue.pending_set();
    pending.union(&proc_pending);

    UserPtr::new(set_ptr).write_bytes(&vmar, &encode_sigset(&pending)[..sigsetsize])?;
    Ok(0)
}
