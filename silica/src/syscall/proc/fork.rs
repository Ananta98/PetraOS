// SPDX-License-Identifier: GPL-2.0

use ostd::arch::cpu::context::UserContext;
use crate::{
    api::errno::{Errno, Result},
    proc::{
        lifecycle::{clone_proc, fork1, CLONE_VFORK, CLONE_VM},
        thread::Thread,
    },
};

pub fn sys_fork(ctx: &mut UserContext) -> Result<usize> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;
    let child_pid = fork1(&td, Some(ctx))?;
    Ok(child_pid.as_u32() as usize)
}

/// `vfork(2)` - creates child process and blocks caller until child execs or exits.
pub fn sys_vfork(ctx: &mut UserContext) -> Result<usize> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;
    let (_child_proc, _child_thread, child_pid) =
        clone_proc(&td, CLONE_VFORK | CLONE_VM, 0, 0, 0, Some(ctx))?;
    Ok(child_pid.as_u32() as usize)
}
