// SPDX-License-Identifier: GPL-2.0

//! `kill(2)` and `tgkill(2)` system calls (Syscalls #62 and #234 on x86_64).

use alloc::vec::Vec;
use ostd::arch::cpu::context::UserContext;

use crate::{
    api::{
        errno::{Errno, Result},
        signal::{SigInfo, Signal},
    },
    proc::{
        Proc,
        thread::Thread,
    },
};

/// `kill(2)` (Syscall #62 on x86_64).
pub fn sys_kill(pid: i32, sig: i32, _ctx: &mut UserContext) -> Result<usize> {
    let sig = Signal::from_u32(sig as u32).ok_or(Errno::EINVAL)?;
    let probe_only = sig.as_u32() == 0;

    if pid > 0 {
        let proc = crate::proc::tree::allproc_find(
            crate::proc::tree::Pid::from_u32(pid as u32).ok_or(Errno::EINVAL)?,
        )
        .ok_or(Errno::ESRCH)?;
        if probe_only {
            return Ok(0);
        }
        send_to(&proc, sig, 0)?;
        return Ok(0);
    }

    let caller = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let wanted_group = if pid == 0 {
        caller.pgid()
    } else {
        crate::proc::tree::Pid::from_u32(pid.unsigned_abs()).ok_or(Errno::ESRCH)?
    };

    let targets: Vec<_> = crate::proc::tree::allproc_iter()
        .filter(|p| p.pgid() == wanted_group)
        .collect();

    let mut found = false;
    for proc in targets.iter() {
        found |= send_to(proc, sig, caller.pid.as_u32()).is_ok();
    }
    if !found {
        return Err(Errno::ESRCH);
    }
    Ok(0)
}

fn send_to(proc: &Proc, sig: Signal, sender: u32) -> Result<()> {
    if sig.as_u32() == 0 {
        return Ok(());
    }
    proc.send_signal(SigInfo::from_user(sig, sender), sender)
}

/// `tgkill(2)` (Syscall #234 on x86_64): signal one thread of a process.
pub fn sys_tgkill(tgid: i32, tid: i32, sig: i32, _ctx: &mut UserContext) -> Result<usize> {
    let sig = Signal::from_u32(sig as u32).ok_or(Errno::EINVAL)?;
    let proc = crate::proc::tree::allproc_find(
        crate::proc::tree::Pid::from_u32(tgid as u32).ok_or(Errno::EINVAL)?,
    )
    .ok_or(Errno::ESRCH)?;

    let tid = crate::proc::thread::Tid::from_u32(tid as u32).ok_or(Errno::EINVAL)?;
    let target = proc
        .inner
        .lock()
        .threads
        .iter()
        .find(|t| t.tid == tid)
        .cloned()
        .ok_or(Errno::ESRCH)?;

    target.post_signal(SigInfo::from_user(
        sig,
        Thread::current_proc().map(|p| p.pid.as_u32()).unwrap_or(0),
    ));
    Ok(0)
}
