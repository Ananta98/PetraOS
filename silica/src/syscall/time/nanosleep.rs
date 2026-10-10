// SPDX-License-Identifier: GPL-2.0

//! `nanosleep(2)` system call (Syscall #35 on x86_64).

use core::time::Duration;
use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::{Thread, ThreadFlags},
    syscall::time::Timespec,
    utils::userspace::user_ptr::UserPtr,
};

/// High-resolution sleep for the specified duration.
pub fn sys_nanosleep(req_ptr: usize, rem_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    if req_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let mut raw = [0u8; 16];
    UserPtr::new(req_ptr).read_bytes(&vmar, &mut raw)?;
    let req = Timespec::from_bytes(&raw).ok_or(Errno::EINVAL)?;

    if req.tv_sec < 0 || req.tv_nsec < 0 || req.tv_nsec >= 1_000_000_000 {
        return Err(Errno::EINVAL);
    }

    if req.tv_sec == 0 && req.tv_nsec == 0 {
        ostd::task::Task::yield_now();
        return Ok(0);
    }

    let start = ostd::timer::Jiffies::elapsed().as_duration();
    let sleep_dur = Duration::new(req.tv_sec as u64, req.tv_nsec as u32);
    let end = start.saturating_add(sleep_dur);

    loop {
        let now = ostd::timer::Jiffies::elapsed().as_duration();
        if now >= end {
            break;
        }

        if let Some(td) = Thread::current()
            && td.inner.lock().flags.contains(ThreadFlags::TDF_ASTPENDING)
        {
            if rem_ptr != 0 {
                let remaining = if end > now {
                    end - now
                } else {
                    Duration::ZERO
                };
                let rem_ts = Timespec {
                    tv_sec: remaining.as_secs() as i64,
                    tv_nsec: remaining.subsec_nanos() as i64,
                };
                UserPtr::new(rem_ptr).write_bytes(&vmar, &rem_ts.to_bytes())?;
            }
            return Err(Errno::EINTR);
        }

        ostd::task::Task::yield_now();
    }

    Ok(0)
}
