// SPDX-License-Identifier: GPL-2.0

//! `nanosleep(2)` system call (Syscall #35 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::Thread,
    time::Timespec,
    uapi::errno::{Errno, Result},
    utils::UserPtr,
};

/// High-resolution sleep for the specified duration.
pub fn sys_nanosleep(req_ptr: usize, rem_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let mut raw = [0u8; 16];
    UserPtr::new(req_ptr).read_bytes(&vmar, &mut raw)?;
    let req = Timespec::from_bytes(&raw).ok_or(Errno::EINVAL)?;

    if !req.is_valid() {
        return Err(Errno::EINVAL);
    }

    if req.tv_sec == 0 && req.tv_nsec == 0 {
        ostd::task::Task::yield_now();
        return Ok(0);
    }

    let sleep_ns = req.total_nanos().ok_or(Errno::EINVAL)?;
    let start_ns = crate::time::get_monotonic_ns();
    let end_ns = start_ns.saturating_add(sleep_ns);

    let (waiter, _) = ostd::sync::Waiter::new_pair();
    let waker = waiter.waker();
    let ktimer = crate::kpi::time::arm(
        core::time::Duration::from_nanos(sleep_ns),
        Some(alloc::boxed::Box::new(move || {
            waker.wake_up();
        })),
    );

    let cond = || {
        let now_ns = crate::time::get_monotonic_ns();
        if now_ns >= end_ns {
            Some(())
        } else {
            None
        }
    };

    let cancel = || {
        if Thread::should_interrupt() {
            Err(Errno::EINTR)
        } else {
            Ok(())
        }
    };

    let res = waiter.wait_until_or_cancelled(cond, cancel);
    ktimer.cancel();

    if let Err(Errno::EINTR) = res {
        if rem_ptr != 0 {
            let now_ns = crate::time::get_monotonic_ns();
            let remaining_ns = end_ns.saturating_sub(now_ns);
            let rem_ts = Timespec {
                tv_sec: (remaining_ns / 1_000_000_000) as i64,
                tv_nsec: (remaining_ns % 1_000_000_000) as i64,
            };
            UserPtr::new(rem_ptr).write_bytes(&vmar, &rem_ts.to_bytes())?;
        }
        return Err(Errno::EINTR);
    }

    Ok(0)
}
