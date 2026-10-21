// SPDX-License-Identifier: GPL-2.0

//! `rt_sigprocmask(2)` system call (Syscall #14 on x86_64).

use ostd::arch::cpu::context::UserContext;

use super::utils::{all_signals, decode_sigset, encode_sigset};
use crate::{
    api::{
        errno::{Errno, Result},
        signal::{SIGSET_WORDS, SigSet},
    },
    proc::thread::{Thread, ThreadFlags},
    utils::userspace::user_ptr::UserPtr,
};

const SIG_BLOCK: i32 = 0;
const SIG_UNBLOCK: i32 = 1;
const SIG_SETMASK: i32 = 2;

/// `rt_sigprocmask(2)`.
pub fn sys_rt_sigprocmask(
    how: i32,
    set_ptr: usize,
    old_set_ptr: usize,
    sigsetsize: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;

    if sigsetsize != 8 && sigsetsize != SIGSET_WORDS * 8 {
        return Err(Errno::EINVAL);
    }

    let vmar = td.proc().ok_or(Errno::ESRCH)?.vmspace();

    let mut inner = td.inner.lock();
    let previous = inner.sigmask;

    let requested = if set_ptr == 0 {
        SigSet::empty()
    } else {
        let mut buf = [0u8; SIGSET_WORDS * 8];
        UserPtr::new(set_ptr).read_bytes(&vmar, &mut buf[..sigsetsize])?;
        decode_sigset(&buf)
    };

    let updated = match how {
        SIG_BLOCK => {
            let mut m = previous;
            for sig in all_signals() {
                if requested.contains(sig) {
                    m.add(sig);
                }
            }
            m
        }
        SIG_UNBLOCK => {
            let mut m = previous;
            for sig in all_signals() {
                if requested.contains(sig) {
                    m.remove(sig);
                }
            }
            m
        }
        SIG_SETMASK => requested,
        _ => return Err(Errno::EINVAL),
    };

    inner.sigmask = updated;

    if old_set_ptr != 0 {
        UserPtr::new(old_set_ptr).write_bytes(&vmar, &encode_sigset(&previous)[..sigsetsize])?;
    }

    inner.flags.insert(ThreadFlags::TDF_ASTPENDING);
    drop(inner);

    Ok(sigsetsize)
}
