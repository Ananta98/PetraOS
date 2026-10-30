// SPDX-License-Identifier: GPL-2.0

//! `rt_sigreturn(2)` system call (Syscall #15 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::thread::{Thread, ThreadFlags},
    uapi::errno::{Errno, Result},
};

/// `rt_sigreturn(2)`.
pub fn sys_rt_sigreturn(ctx: &mut UserContext) -> Result<usize> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;

    {
        let inner = td.inner.lock();
        if !inner.flags.contains(ThreadFlags::IN_HANDLER) {
            ostd::error!("sys_rt_sigreturn: not IN_HANDLER (tid {})", td.tid);
            return Err(Errno::EFAULT);
        }
    }

    if let Err(err) = td.return_from_handler(ctx) {
        ostd::error!("sys_rt_sigreturn: return_from_handler failed: {:?}", err);
        return Err(err);
    }
    Ok(0)
}
