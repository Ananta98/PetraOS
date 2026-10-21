// SPDX-License-Identifier: GPL-2.0

//! POSIX Interval Timer System Calls:
//! - `timer_create(2)` (Syscall #222)
//! - `timer_settime(2)` (Syscall #223)
//! - `timer_gettime(2)` (Syscall #224)
//! - `timer_getoverrun(2)` (Syscall #225)
//! - `timer_delete(2)` (Syscall #226)

use alloc::sync::Arc;
use core::time::Duration;
use ostd::arch::cpu::context::UserContext;

use crate::{
    api::{
        errno::{Errno, Result},
        signal::Signal,
    },
    proc::thread::{Thread, Tid},
    time::{
        CLOCK_BOOTTIME, CLOCK_BOOTTIME_ALARM, CLOCK_MONOTONIC, CLOCK_MONOTONIC_COARSE,
        CLOCK_MONOTONIC_RAW, CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME, CLOCK_REALTIME_ALARM,
        CLOCK_REALTIME_COARSE, CLOCK_TAI, CLOCK_THREAD_CPUTIME_ID, Itimerspec, Timespec,
        posix_timer::{
            PosixTimer, SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD, SIGEV_THREAD_ID, get_clock_now,
            on_timer_expire,
        },
    },
    utils::userspace::user_ptr::UserPtr,
};

use super::clock::TIMER_ABSTIME;

/// Creates an interval timer (`timer_create(2)`, Syscall #222).
pub fn sys_timer_create(
    clock_id: i32,
    sevp_ptr: usize,
    timerid_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    match clock_id {
        CLOCK_REALTIME
        | CLOCK_MONOTONIC
        | CLOCK_PROCESS_CPUTIME_ID
        | CLOCK_THREAD_CPUTIME_ID
        | CLOCK_MONOTONIC_RAW
        | CLOCK_REALTIME_COARSE
        | CLOCK_MONOTONIC_COARSE
        | CLOCK_BOOTTIME
        | CLOCK_REALTIME_ALARM
        | CLOCK_BOOTTIME_ALARM
        | CLOCK_TAI => {}
        _ => return Err(Errno::EINVAL),
    }

    if timerid_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let (sigev_notify, sigev_signo, sigev_value, sigev_tid) = if sevp_ptr != 0 {
        let mut buf = [0u8; 64];
        UserPtr::new(sevp_ptr).read_bytes(&vmar, &mut buf)?;
        let sigev_value = u64::from_ne_bytes(buf[0..8].try_into().unwrap());
        let sigev_signo = i32::from_ne_bytes(buf[8..12].try_into().unwrap());
        let sigev_notify = i32::from_ne_bytes(buf[12..16].try_into().unwrap());
        let tid_raw = i32::from_ne_bytes(buf[16..20].try_into().unwrap());

        match sigev_notify {
            SIGEV_SIGNAL => {
                if sigev_signo <= 0
                    || sigev_signo > 64
                    || Signal::from_u32(sigev_signo as u32).is_none()
                {
                    return Err(Errno::EINVAL);
                }
                (SIGEV_SIGNAL, sigev_signo, sigev_value, None)
            }
            SIGEV_NONE => (SIGEV_NONE, 0, sigev_value, None),
            SIGEV_THREAD_ID => {
                if sigev_signo <= 0
                    || sigev_signo > 64
                    || Signal::from_u32(sigev_signo as u32).is_none()
                {
                    return Err(Errno::EINVAL);
                }
                let tid = Tid::from_u32(tid_raw as u32).ok_or(Errno::EINVAL)?;
                let inner = proc.inner.lock();
                if !inner.threads.iter().any(|t| t.tid == tid) {
                    return Err(Errno::EINVAL);
                }
                drop(inner);
                (SIGEV_THREAD_ID, sigev_signo, sigev_value, Some(tid))
            }
            SIGEV_THREAD => {
                // If user space passes SIGEV_THREAD directly to the kernel, accept it
                // instead of failing with EINVAL. Deliver signal if signo is valid,
                // or treat as SIGEV_NONE if 0.
                let signo = if sigev_signo > 0 && sigev_signo <= 64 {
                    sigev_signo
                } else {
                    Signal::SIGALRM.as_u32() as i32
                };
                (SIGEV_SIGNAL, signo, sigev_value, None)
            }
            _ => return Err(Errno::EINVAL),
        }
    } else {
        (SIGEV_SIGNAL, Signal::SIGALRM.as_u32() as i32, 0, None)
    };

    let mut timers = proc.posix_timers.lock();
    let mut id = 0i32;
    while timers.contains_key(&id) {
        id = id.checked_add(1).ok_or(Errno::EAGAIN)?;
    }
    if timers.len() >= 512 {
        return Err(Errno::EAGAIN);
    }

    let final_sigev_value = if sevp_ptr == 0 {
        id as u64
    } else {
        sigev_value
    };

    let timer = Arc::new(PosixTimer::new(
        id,
        clock_id,
        sigev_notify,
        sigev_signo,
        final_sigev_value,
        sigev_tid,
        proc.pid,
    ));
    timers.insert(id, timer);
    drop(timers);

    let res = UserPtr::new(timerid_ptr).write_bytes(&vmar, &id.to_ne_bytes());
    if res.is_err() {
        proc.posix_timers.lock().remove(&id);
    }
    res?;

    Ok(0)
}

/// Arms or disarms an interval timer (`timer_settime(2)`, Syscall #223).
pub fn sys_timer_settime(
    timerid: i32,
    flags: i32,
    new_val_ptr: usize,
    old_val_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if new_val_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    if (flags & !TIMER_ABSTIME) != 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let mut new_buf = [0u8; 32];
    UserPtr::new(new_val_ptr).read_bytes(&vmar, &mut new_buf)?;
    let new_val = Itimerspec::from_bytes(&new_buf).ok_or(Errno::EINVAL)?;

    if !new_val.it_interval.is_valid() || !new_val.it_value.is_valid() {
        return Err(Errno::EINVAL);
    }

    let timer = proc
        .posix_timers
        .lock()
        .get(&timerid)
        .cloned()
        .ok_or(Errno::EINVAL)?;

    // If caller requested old_value, write it out before applying changes
    if old_val_ptr != 0 {
        let inner = timer.inner.lock();
        let old_interval = Timespec {
            tv_sec: (inner.interval_ns / 1_000_000_000) as i64,
            tv_nsec: (inner.interval_ns % 1_000_000_000) as i64,
        };
        let old_value = if inner.armed {
            let clock_now = get_clock_now(timer.clock_id);
            let rem_ns = inner.next_deadline_ns.saturating_sub(clock_now);
            Timespec {
                tv_sec: (rem_ns / 1_000_000_000) as i64,
                tv_nsec: (rem_ns % 1_000_000_000) as i64,
            }
        } else {
            Timespec::ZERO
        };
        drop(inner);

        let old_itimerspec = Itimerspec {
            it_interval: old_interval,
            it_value: old_value,
        };
        UserPtr::new(old_val_ptr).write_bytes(&vmar, &old_itimerspec.to_bytes())?;
    }

    let mut inner = timer.inner.lock();
    inner.generation = inner.generation.wrapping_add(1);
    let generation = inner.generation;

    let val_ns = new_val.it_value.total_nanos().ok_or(Errno::EINVAL)?;
    let interval_ns = new_val.it_interval.total_nanos().ok_or(Errno::EINVAL)?;

    if val_ns == 0 {
        // Disarm
        inner.armed = false;
        inner.interval_ns = 0;
        inner.next_deadline_ns = 0;
        return Ok(0);
    }

    // Arm
    inner.armed = true;
    inner.interval_ns = interval_ns;

    let is_abstime = (flags & TIMER_ABSTIME) != 0;
    let clock_now = get_clock_now(timer.clock_id);

    let (delay_ns, target_ns) = if is_abstime {
        let target = val_ns;
        let delay = target.saturating_sub(clock_now);
        (delay, target)
    } else {
        (val_ns, clock_now.saturating_add(val_ns))
    };

    inner.next_deadline_ns = target_ns;
    drop(inner);

    let timer_clone = Arc::clone(&timer);
    let proc_id = proc.pid;

    if delay_ns == 0 {
        crate::kpi::task::schedule_work_fn(move || {
            on_timer_expire(timer_clone, proc_id, generation);
        });
    } else {
        crate::kpi::task::schedule_delayed_work_fn(
            move || {
                on_timer_expire(timer_clone, proc_id, generation);
            },
            Duration::from_nanos(delay_ns),
        );
    }

    Ok(0)
}

/// Retrieves the remaining time and interval of an interval timer (`timer_gettime(2)`, Syscall #224).
pub fn sys_timer_gettime(
    timerid: i32,
    curr_val_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if curr_val_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let timer = proc
        .posix_timers
        .lock()
        .get(&timerid)
        .cloned()
        .ok_or(Errno::EINVAL)?;

    let inner = timer.inner.lock();
    let it_interval = Timespec {
        tv_sec: (inner.interval_ns / 1_000_000_000) as i64,
        tv_nsec: (inner.interval_ns % 1_000_000_000) as i64,
    };
    let it_value = if inner.armed {
        let clock_now = get_clock_now(timer.clock_id);
        let rem_ns = inner.next_deadline_ns.saturating_sub(clock_now);
        Timespec {
            tv_sec: (rem_ns / 1_000_000_000) as i64,
            tv_nsec: (rem_ns % 1_000_000_000) as i64,
        }
    } else {
        Timespec::ZERO
    };
    drop(inner);

    let it = Itimerspec {
        it_interval,
        it_value,
    };
    UserPtr::new(curr_val_ptr).write_bytes(&vmar, &it.to_bytes())?;
    Ok(0)
}

/// Retrieves the overrun count of an interval timer (`timer_getoverrun(2)`, Syscall #225).
pub fn sys_timer_getoverrun(timerid: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let timer = proc
        .posix_timers
        .lock()
        .get(&timerid)
        .cloned()
        .ok_or(Errno::EINVAL)?;

    let inner = timer.inner.lock();
    let overrun = inner.overrun;
    Ok(overrun as usize)
}

/// Deletes an interval timer (`timer_delete(2)`, Syscall #226).
pub fn sys_timer_delete(timerid: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let timer = proc
        .posix_timers
        .lock()
        .remove(&timerid)
        .ok_or(Errno::EINVAL)?;

    let mut inner = timer.inner.lock();
    inner.armed = false;
    inner.generation = inner.generation.wrapping_add(1);
    Ok(0)
}
