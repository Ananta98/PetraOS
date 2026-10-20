// SPDX-License-Identifier: GPL-2.0

//! Job control and process grouping system calls (POSIX `setpgid`, `getpgrp`, `setsid`).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::{Pid, allproc_find, thread::Thread},
};

/// `setpgid(2)` - sets process group ID for process.
pub fn sys_setpgid(pid: i32, pgid: i32, _ctx: &mut UserContext) -> Result<usize> {
    let caller = Thread::current_proc().ok_or(Errno::ESRCH)?;

    let target = if pid == 0 || pid as u32 == caller.pid.as_u32() {
        caller
    } else {
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        let target_pid = Pid::from_u32(pid as u32).ok_or(Errno::ESRCH)?;
        allproc_find(target_pid).ok_or(Errno::ESRCH)?
    };

    let new_pgid = if pgid == 0 {
        target.pid
    } else {
        if pgid < 0 {
            return Err(Errno::EINVAL);
        }
        Pid::from_u32(pgid as u32).ok_or(Errno::EINVAL)?
    };

    target.set_pgid(new_pgid)?;
    Ok(0)
}

/// `getpgrp(2)` - returns current process group ID.
pub fn sys_getpgrp(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    Ok(proc.pgid().as_u32() as usize)
}

/// `getpgid(2)` - returns process group ID of target process.
pub fn sys_getpgid(pid: i32, _ctx: &mut UserContext) -> Result<usize> {
    let target = if pid == 0 {
        Thread::current_proc().ok_or(Errno::ESRCH)?
    } else {
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        let target_pid = Pid::from_u32(pid as u32).ok_or(Errno::ESRCH)?;
        allproc_find(target_pid).ok_or(Errno::ESRCH)?
    };

    Ok(target.pgid().as_u32() as usize)
}

/// `setsid(2)` - creates a new session and sets process group ID.
pub fn sys_setsid(_ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let sid = proc.setsid()?;
    Ok(sid.as_u32() as usize)
}

/// `getsid(2)` - returns session ID of target process.
pub fn sys_getsid(pid: i32, _ctx: &mut UserContext) -> Result<usize> {
    let target = if pid == 0 {
        Thread::current_proc().ok_or(Errno::ESRCH)?
    } else {
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        let target_pid = Pid::from_u32(pid as u32).ok_or(Errno::ESRCH)?;
        allproc_find(target_pid).ok_or(Errno::ESRCH)?
    };

    Ok(target.sid().as_u32() as usize)
}
