// SPDX-License-Identifier: GPL-2.0

//! Signal transmission system calls (`kill`, `tgkill`).

use alloc::vec::Vec;
use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::{
        Pid, Proc, allproc_find, allproc_for_each, pgrp_members,
        thread::{Thread, Tid},
    },
    uapi::{
        errno::{Errno, Result},
        signal::{SigInfo, Signal},
    },
};

/// `kill(2)` (Syscall #62 on x86_64).
pub fn sys_kill(pid: i32, sig: i32, _ctx: &mut UserContext) -> Result<usize> {
    if sig < 0 || sig > 64 {
        return Err(Errno::EINVAL);
    }
    let probe_only = sig == 0;
    let sig_opt = if probe_only {
        None
    } else {
        Some(Signal::from_u32(sig as u32).ok_or(Errno::EINVAL)?)
    };

    if pid > 0 {
        let proc =
            allproc_find(Pid::from_u32(pid as u32).ok_or(Errno::EINVAL)?).ok_or(Errno::ESRCH)?;
        if let Some(sig) = sig_opt {
            send_to(&proc, sig, 0)?;
        }
        return Ok(0);
    }

    let caller = Thread::proc_or_esrch()?;
    let wanted_group = if pid == 0 {
        caller.pgid()
    } else {
        Pid::from_u32(pid.unsigned_abs()).ok_or(Errno::ESRCH)?
    };

    let targets: Vec<_> = pgrp_members(wanted_group);
    if targets.is_empty() {
        return Err(Errno::ESRCH);
    }

    if let Some(sig) = sig_opt {
        let mut found = false;
        for proc in targets.iter() {
            found |= send_to(proc, sig, caller.pid.as_u32()).is_ok();
        }
        if !found {
            return Err(Errno::ESRCH);
        }
    }
    Ok(0)
}

fn send_to(proc: &Proc, sig: Signal, sender: u32) -> Result<()> {
    if sig.as_u32() == 0 {
        return Ok(());
    }
    proc.send_signal(SigInfo::from_user(sig, sender), sender)
}

/// `tkill(2)` (Syscall #200 on x86_64): signal one thread.
pub fn sys_tkill(tid: i32, sig: i32, _ctx: &mut UserContext) -> Result<usize> {
    if tid <= 0 || sig < 0 || sig > 64 {
        return Err(Errno::EINVAL);
    }

    let tid_val = Tid::from_u32(tid as u32).ok_or(Errno::EINVAL)?;
    let pid_val = Pid::from_u32(tid as u32).ok_or(Errno::EINVAL)?;

    // Check direct process first (main thread tid == pid)
    let mut target = None;
    if let Some(proc) = allproc_find(pid_val) {
        let inner = proc.inner.lock();
        if let Some(t) = inner.threads.iter().find(|t| t.tid == tid_val).cloned() {
            target = Some(t);
        }
    }

    // Otherwise check threads across all processes
    if target.is_none() {
        allproc_for_each(|proc| {
            if target.is_some() {
                return;
            }
            let inner = proc.inner.lock();
            if let Some(t) = inner.threads.iter().find(|t| t.tid == tid_val).cloned() {
                target = Some(t);
            }
        });
    }

    let target = target.ok_or(Errno::ESRCH)?;

    if sig == 0 {
        return Ok(0);
    }

    let sig = Signal::from_u32(sig as u32).ok_or(Errno::EINVAL)?;
    target.post_signal(SigInfo::from_user(
        sig,
        Thread::current_proc().map(|p| p.pid.as_u32()).unwrap_or(0),
    ));
    Ok(0)
}

/// `tgkill(2)` (Syscall #234 on x86_64): signal one thread of a process.
pub fn sys_tgkill(tgid: i32, tid: i32, sig: i32, _ctx: &mut UserContext) -> Result<usize> {
    if sig < 0 || sig > 64 {
        return Err(Errno::EINVAL);
    }
    let proc =
        allproc_find(Pid::from_u32(tgid as u32).ok_or(Errno::EINVAL)?).ok_or(Errno::ESRCH)?;

    let tid = Tid::from_u32(tid as u32).ok_or(Errno::EINVAL)?;
    let target = proc
        .inner
        .lock()
        .threads
        .iter()
        .find(|t| t.tid == tid)
        .cloned()
        .ok_or(Errno::ESRCH)?;

    if sig == 0 {
        return Ok(0);
    }

    let sig = Signal::from_u32(sig as u32).ok_or(Errno::EINVAL)?;
    target.post_signal(SigInfo::from_user(
        sig,
        Thread::current_proc().map(|p| p.pid.as_u32()).unwrap_or(0),
    ));
    Ok(0)
}
