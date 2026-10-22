// SPDX-License-Identifier: GPL-2.0

//! Process priority (`getpriority`, `setpriority`) and round-robin quantum system calls.

use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::arch::cpu::context::UserContext;

use super::param::resolve_target_proc;
use crate::uapi::cred::Uid;
use crate::uapi::errno::{Errno, Result};
use crate::proc::Proc;
use crate::proc::thread::Thread;
use crate::proc::{Pid, allproc_iter, pgrp_members};
use crate::syscall::time::Timespec;
use crate::utils::userspace::UserPtr;

/// Target which processes by PID.
pub const PRIO_PROCESS: i32 = 0;
/// Target all processes in a process group by PGID.
pub const PRIO_PGRP: i32 = 1;
/// Target all processes owned by a user by UID.
pub const PRIO_USER: i32 = 2;

/// Minimum nice value (highest priority).
pub const PRIO_MIN: i32 = -20;
/// Maximum nice value (lowest priority).
pub const PRIO_MAX: i32 = 19;

/// Resolves matching processes according to `(which, who)`.
fn find_matching_procs(caller: &Arc<Proc>, which: i32, who: i32) -> Result<Vec<Arc<Proc>>> {
    match which {
        PRIO_PROCESS => {
            if who < 0 {
                return Err(Errno::EINVAL);
            }
            let target = resolve_target_proc(caller, who)?;
            Ok(alloc::vec![target])
        }
        PRIO_PGRP => {
            if who < 0 {
                return Err(Errno::EINVAL);
            }
            let target_pgid = if who == 0 {
                caller.pgid()
            } else {
                Pid::from_u32(who as u32).ok_or(Errno::ESRCH)?
            };

            let procs = pgrp_members(target_pgid);
            if procs.is_empty() {
                return Err(Errno::ESRCH);
            }
            Ok(procs)
        }
        PRIO_USER => {
            if who < 0 {
                return Err(Errno::EINVAL);
            }
            let target_uid = if who == 0 {
                caller.cred().cr_uid
            } else {
                Uid(who as u32)
            };

            let procs: Vec<Arc<Proc>> = allproc_iter()
                .into_iter()
                .filter(|p| p.cred().cr_uid == target_uid)
                .collect();

            if procs.is_empty() {
                return Err(Errno::ESRCH);
            }
            Ok(procs)
        }
        _ => Err(Errno::EINVAL),
    }
}

/// `getpriority(2)` - Returns highest priority (lowest numerical nice value)
/// of specified processes.
///
/// In the Linux kernel ABI, the return value on success is `20 - nice` (range 1..=40).
pub fn sys_getpriority(which: i32, who: i32, _ctx: &mut UserContext) -> Result<usize> {
    let caller = Thread::proc_or_esrch()?;
    let procs = find_matching_procs(&caller, which, who)?;

    let mut min_nice = i32::MAX;
    let mut has_perm = false;

    for p in &procs {
        if crate::sched::check_sched_permission(&caller, p).is_ok() {
            has_perm = true;
            let nice = crate::sched::get_sched_state(p.pid).nice;
            if nice < min_nice {
                min_nice = nice;
            }
        }
    }

    if !has_perm {
        return Err(Errno::EPERM);
    }

    let min_nice = min_nice.clamp(PRIO_MIN, PRIO_MAX);
    Ok((20 - min_nice) as usize)
}

/// `setpriority(2)` - Sets scheduling priority (nice value) for specified processes.
pub fn sys_setpriority(which: i32, who: i32, prio: i32, _ctx: &mut UserContext) -> Result<usize> {
    let caller = Thread::proc_or_esrch()?;
    let procs = find_matching_procs(&caller, which, who)?;

    let target_nice = prio.clamp(PRIO_MIN, PRIO_MAX);
    let is_superuser = caller.cred().is_superuser();

    // Verify permissions for all targets first
    for p in &procs {
        crate::sched::check_sched_permission(&caller, p)?;

        let current_nice = crate::sched::get_sched_state(p.pid).nice;
        if target_nice < current_nice && !is_superuser {
            return Err(Errno::EACCES);
        }
    }

    // Apply priority updates
    for p in &procs {
        crate::sched::update_sched_state(p.pid, |s| {
            s.nice = target_nice;
        });
        crate::sched::apply_sched_state_to_proc(p);
    }

    Ok(0)
}

/// `sched_rr_get_interval(2)` - Retrieves round-robin time quantum for a process.
pub fn sys_sched_rr_get_interval(pid: i32, tp_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let caller = Thread::proc_or_esrch()?;
    let target = resolve_target_proc(&caller, pid)?;
    crate::sched::check_sched_permission(&caller, &target)?;

    // Return the configured real-time round-robin slice (100 ms).
    let slice_ns = crate::sched::DEF_RT_TIMESLICE_NS;
    let ts = Timespec {
        tv_sec: (slice_ns / 1_000_000_000) as i64,
        tv_nsec: (slice_ns % 1_000_000_000) as i64,
    };

    let vmar = caller.vmspace();
    UserPtr::new(tp_ptr).write_bytes(&vmar, &ts.to_bytes())?;
    Ok(0)
}
