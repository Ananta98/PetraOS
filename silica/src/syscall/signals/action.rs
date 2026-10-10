// SPDX-License-Identifier: GPL-2.0

//! `rt_sigaction(2)` system call (Syscall #13 on x86_64).

use ostd::arch::cpu::context::UserContext;

use super::utils::{decode_sigaction, encode_sigaction, SIGACTION_LEN};
use crate::{
    api::{
        errno::{Errno, Result},
        signal::{SIGSET_WORDS, Signal},
    },
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
};

/// `rt_sigaction(2)`.
pub fn sys_rt_sigaction(
    sig: i32,
    act_ptr: usize,
    old_act_ptr: usize,
    sigsetsize: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let sig = Signal::from_u32(sig as u32).ok_or(Errno::EINVAL)?;

    if sigsetsize != SIGSET_WORDS * 8 {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let mut new_action = if act_ptr == 0 {
        None
    } else {
        let mut buf = [0u8; SIGACTION_LEN];
        UserPtr::new(act_ptr).read_bytes(&vmar, &mut buf)?;
        Some(decode_sigaction(&buf))
    };

    if let Some(action) = new_action {
        let mut action = action;
        action.sa_restorer = 0;
        new_action = Some(action);
    }

    let old = {
        let mut sigacts = proc.sigacts.lock();
        let old = sigacts.get(sig);
        if let Some(action) = new_action {
            sigacts.set(sig, action);
        }
        old
    };

    if old_act_ptr != 0 {
        UserPtr::new(old_act_ptr).write_bytes(&vmar, &encode_sigaction(&old))?;
    }

    Ok(SIGSET_WORDS * 8)
}
