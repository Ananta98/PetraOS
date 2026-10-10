// SPDX-License-Identifier: GPL-2.0

//! `rt_sigreturn(2)` system call (Syscall #15 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::{Thread, ThreadFlags},
};

/// `rt_sigreturn(2)`.
pub fn sys_rt_sigreturn(ctx: &mut UserContext) -> Result<usize> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;

    {
        let inner = td.inner.lock();
        if !inner.flags.contains(ThreadFlags::IN_HANDLER) {
            return Err(Errno::EFAULT);
        }
    }

    td.return_from_handler(ctx)?;
    Ok(0)
}
