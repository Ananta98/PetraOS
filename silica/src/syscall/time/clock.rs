// SPDX-License-Identifier: GPL-2.0

//! POSIX Clock and Time System Calls:
//! - `clock_gettime(2)` (Syscall #228)
//! - `clock_getres(2)` (Syscall #229)
//! - `clock_settime(2)` (Syscall #227)
//! - `clock_nanosleep(2)` (Syscall #230)
//! - `time(2)` (Syscall #201)

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    time::{
        CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME, CLOCK_THREAD_CPUTIME_ID,
        Timespec,
    },
    utils::userspace::user_ptr::UserPtr,
};

pub const TIMER_ABSTIME: i32 = 1;

/// Retrieves the time of the specified clock (`clock_gettime(2)`, Syscall #228).
pub fn sys_clock_gettime(clock_id: i32, tp_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    if tp_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let ts = match clock_id {
        CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            let td = Thread::current().ok_or(Errno::ESRCH)?;
            let exec_ns = crate::sched::stats::for_task(&td.td_kstack)
                .map(|s| s.exec_ns)
                .unwrap_or(0);
            Timespec {
                tv_sec: (exec_ns / 1_000_000_000) as i64,
                tv_nsec: (exec_ns % 1_000_000_000) as i64,
            }
        }
        _ => crate::time::get_timespec(clock_id)?,
    };

    UserPtr::new(tp_ptr).write_bytes(&vmar, &ts.to_bytes())?;
    Ok(0)
}

/// Finds the resolution of the specified clock (`clock_getres(2)`, Syscall #229).
pub fn sys_clock_getres(clock_id: i32, res_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let res = crate::time::get_resolution(clock_id)?;

    if res_ptr != 0 {
        let proc = Thread::proc_or_esrch()?;
        let vmar = proc.vmspace();
        UserPtr::new(res_ptr).write_bytes(&vmar, &res.to_bytes())?;
    }

    Ok(0)
}

/// Sets the time of the specified clock (`clock_settime(2)`, Syscall #227).
pub fn sys_clock_settime(clock_id: i32, tp_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    if tp_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    // Only root may set the system time
    if !proc.cred().cr_uid.is_root() {
        return Err(Errno::EPERM);
    }

    // Linux only allows setting CLOCK_REALTIME
    if clock_id != CLOCK_REALTIME {
        return Err(Errno::EINVAL);
    }

    let mut buf = [0u8; 16];
    UserPtr::new(tp_ptr).read_bytes(&vmar, &mut buf)?;
    let new_ts = Timespec::from_bytes(&buf).ok_or(Errno::EINVAL)?;

    if !new_ts.is_valid() {
        return Err(Errno::EINVAL);
    }

    let new_total_ns = new_ts.total_nanos().ok_or(Errno::EINVAL)?;
    let current_ns = crate::time::get_realtime_ns();
    let delta_ns = (new_total_ns as i128) - (current_ns as i128);

    crate::time::adjust_realtime_offset(delta_ns as i64);
    Ok(0)
}

/// High-resolution sleep with a specified clock (`clock_nanosleep(2)`, Syscall #230).
pub fn sys_clock_nanosleep(
    clock_id: i32,
    flags: i32,
    req_ptr: usize,
    rem_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if req_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    // Only CLOCK_REALTIME and CLOCK_MONOTONIC are supported for sleep
    if clock_id != CLOCK_REALTIME && clock_id != CLOCK_MONOTONIC {
        return Err(Errno::ENOTSUP);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let mut raw = [0u8; 16];
    UserPtr::new(req_ptr).read_bytes(&vmar, &mut raw)?;
    let req = Timespec::from_bytes(&raw).ok_or(Errno::EINVAL)?;

    if !req.is_valid() {
        return Err(Errno::EINVAL);
    }

    let is_abstime = (flags & TIMER_ABSTIME) != 0;
    let target_ns = if is_abstime {
        req.total_nanos().ok_or(Errno::EINVAL)?
    } else {
        let req_ns = req.total_nanos().ok_or(Errno::EINVAL)?;
        let now_ns = if clock_id == CLOCK_REALTIME {
            crate::time::get_realtime_ns()
        } else {
            crate::time::get_monotonic_ns()
        };
        now_ns.saturating_add(req_ns)
    };

    loop {
        let now_ns = if clock_id == CLOCK_REALTIME {
            crate::time::get_realtime_ns()
        } else {
            crate::time::get_monotonic_ns()
        };

        if now_ns >= target_ns {
            break;
        }

        // Check for pending signal
        if Thread::should_interrupt() {
            // Only write remaining time for relative sleeps
            if !is_abstime && rem_ptr != 0 {
                let remaining_ns = target_ns.saturating_sub(now_ns);
                let rem_ts = Timespec {
                    tv_sec: (remaining_ns / 1_000_000_000) as i64,
                    tv_nsec: (remaining_ns % 1_000_000_000) as i64,
                };
                UserPtr::new(rem_ptr).write_bytes(&vmar, &rem_ts.to_bytes())?;
            }
            return Err(Errno::EINTR);
        }

        ostd::task::Task::yield_now();
    }

    Ok(0)
}

/// Returns the current time in seconds since the Epoch (`time(2)`, Syscall #201).
pub fn sys_time(tloc_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let now_sec = (crate::time::get_realtime_ns() / 1_000_000_000) as i64;

    if tloc_ptr != 0 {
        let proc = Thread::proc_or_esrch()?;
        let vmar = proc.vmspace();
        UserPtr::new(tloc_ptr).write_bytes(&vmar, &now_sec.to_ne_bytes())?;
    }

    Ok(now_sec as usize)
}
