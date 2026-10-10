// SPDX-License-Identifier: GPL-2.0

//! `sigaltstack(2)` system call (Syscall #131 on x86_64).

use core::mem::size_of;
use ostd::arch::cpu::context::UserContext;

use super::utils::{decode_sigstack, encode_sigstack};
use crate::{
    api::{
        errno::{Errno, Result},
        signal::SigStack,
    },
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
};

/// `sigaltstack(2)`.
pub fn sys_sigaltstack(ss_ptr: usize, old_ss_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;
    let proc = td.proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let previous = td.inner.lock().altstack;

    if old_ss_ptr != 0 {
        UserPtr::new(old_ss_ptr).write_bytes(&vmar, &encode_sigstack(&previous))?;
    }

    if ss_ptr != 0 {
        let mut buf = [0u8; size_of::<SigStack>()];
        UserPtr::new(ss_ptr).read_bytes(&vmar, &mut buf)?;
        let requested = decode_sigstack(&buf);
        let effective = if requested.ss_sp == 0 || requested.ss_size == 0 {
            SigStack {
                ss_flags: crate::api::signal::ss_flags::DISABLED,
                ..SigStack::default()
            }
        } else {
            requested
        };
        td.inner.lock().altstack = effective;
    }

    Ok(0)
}
