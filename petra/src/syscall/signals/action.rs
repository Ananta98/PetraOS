// SPDX-License-Identifier: GPL-2.0

//! `rt_sigaction(2)` system call (Syscall #13 on x86_64).

use ostd::arch::cpu::context::UserContext;

use super::utils::{KERNEL_SIGSET_LEN, SIGACTION_LEN, decode_sigaction, encode_sigaction};
use crate::{
    proc::Thread,
    uapi::{
        errno::{Errno, Result},
        signal::Signal,
    },
    utils::UserPtr,
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

    if sigsetsize != KERNEL_SIGSET_LEN {
        return Err(Errno::EINVAL);
    }

    // The dispositions of SIGKILL and SIGSTOP are fixed; only querying them is allowed.
    let is_unchangeable = matches!(sig, Signal::SIGKILL | Signal::SIGSTOP);
    if act_ptr != 0 && is_unchangeable {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let new_action = if act_ptr == 0 {
        None
    } else {
        let mut buf = [0u8; SIGACTION_LEN];
        UserPtr::new(act_ptr).read_bytes(&vmar, &mut buf)?;
        Some(decode_sigaction(&buf))
    };

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

    Ok(0)
}
