//! System calls for extensible scheduler attribute management (`sched_setattr`, `sched_getattr`).

use super::types::{
    resolve_target_thread, resolve_target_threads, SchedAttr, SCHED_BATCH, SCHED_DEADLINE,
    SCHED_FIFO, SCHED_IDLE, SCHED_OTHER, SCHED_RR,
};
use crate::arch::syscall::SyscallFrame;
use crate::sched::nice::Nice;
use crate::sched::policy::{RtPriority, SchedPolicy};
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};

/// `sys_sched_getattr` (SYS_SCHED_GETATTR = 315)
///
/// Fetches scheduler attributes including policy, nice, and priority.
pub fn sys_sched_getattr(frame: &mut SyscallFrame) -> SyscallResult {
    let pid = frame.arg1() as i32;
    let attr_ptr = UserPtr::<SchedAttr>::from_u64(frame.arg2());
    let size = frame.arg3() as u32;
    let flags = frame.arg4() as u32;

    if pid < 0 || flags != 0 {
        return Err(SyscallError::EINVAL);
    }
    if size < core::mem::size_of::<SchedAttr>() as u32 {
        return Err(SyscallError::EINVAL);
    }
    if !attr_ptr.is_valid() {
        return Err(SyscallError::EFAULT);
    }

    let target = resolve_target_thread(pid)?;
    let th = target.lock();

    let (raw_policy, raw_prio) = match th.sched_policy {
        SchedPolicy::Fair => (SCHED_OTHER, 0),
        SchedPolicy::Fifo => (SCHED_FIFO, th.rt_priority.value() as u32),
        SchedPolicy::RoundRobin => (SCHED_RR, th.rt_priority.value() as u32),
    };

    let attr = SchedAttr {
        size: core::mem::size_of::<SchedAttr>() as u32,
        sched_policy: raw_policy,
        sched_flags: 0,
        sched_nice: th.nice.value() as i32,
        sched_priority: raw_prio,
        sched_runtime: 0,
        sched_deadline: 0,
        sched_period: 0,
    };

    attr_ptr.write(attr).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}

/// `sys_sched_setattr` (SYS_SCHED_SETATTR = 314)
///
/// Sets scheduler attributes including policy, nice, and priority.
pub fn sys_sched_setattr(frame: &mut SyscallFrame) -> SyscallResult {
    let pid = frame.arg1() as i32;
    let attr_ptr = UserPtr::<SchedAttr>::from_u64(frame.arg2());
    let flags = frame.arg3() as u32;

    if pid < 0 || flags != 0 {
        return Err(SyscallError::EINVAL);
    }

    let attr = attr_ptr.read().ok_or(SyscallError::EFAULT)?;
    if attr.size < 48 || attr.size > 4096 {
        return Err(SyscallError::EINVAL);
    }

    let (policy, rt_prio, opt_nice) = match attr.sched_policy {
        SCHED_OTHER | SCHED_BATCH | SCHED_IDLE => {
            if attr.sched_priority != 0 {
                return Err(SyscallError::EINVAL);
            }
            if attr.sched_nice < -20 || attr.sched_nice > 19 {
                return Err(SyscallError::EINVAL);
            }
            let nice = Nice::new(attr.sched_nice as i8).map_err(|_| SyscallError::EINVAL)?;
            (SchedPolicy::Fair, RtPriority::DEFAULT, Some(nice))
        }
        SCHED_FIFO => {
            if attr.sched_priority < 1 || attr.sched_priority > 99 {
                return Err(SyscallError::EINVAL);
            }
            let prio = RtPriority::new(attr.sched_priority as u8).map_err(|_| SyscallError::EINVAL)?;
            (SchedPolicy::Fifo, prio, None)
        }
        SCHED_RR => {
            if attr.sched_priority < 1 || attr.sched_priority > 99 {
                return Err(SyscallError::EINVAL);
            }
            let prio = RtPriority::new(attr.sched_priority as u8).map_err(|_| SyscallError::EINVAL)?;
            (SchedPolicy::RoundRobin, prio, None)
        }
        SCHED_DEADLINE => return Err(SyscallError::EINVAL),
        _ => return Err(SyscallError::EINVAL),
    };

    let targets = resolve_target_threads(pid)?;
    for thread in targets {
        let mut th = thread.lock();
        th.sched_policy = policy;
        th.rt_priority = rt_prio;
        if let Some(nice) = opt_nice {
            th.nice = nice;
            th.weight = nice.weight();
        }
    }

    Ok(0)
}
