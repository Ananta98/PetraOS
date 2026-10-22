// SPDX-License-Identifier: GPL-2.0

use crate::{
    proc::{
        lifecycle::{CLONE_VFORK, CLONE_VM, clone_proc, fork1},
        thread::Thread,
    },
    uapi::errno::{Errno, Result},
};
use ostd::arch::cpu::context::UserContext;

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
