// SPDX-License-Identifier: GPL-2.0

//! `clock_gettime(2)` system call (Syscall #228 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    syscall::time::{get_boot_epoch_secs, Timespec},
    utils::userspace::user_ptr::UserPtr,
};

pub const CLOCK_REALTIME: i32 = 0;
pub const CLOCK_MONOTONIC: i32 = 1;
pub const CLOCK_PROCESS_CPUTIME_ID: i32 = 2;
pub const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
pub const CLOCK_MONOTONIC_RAW: i32 = 4;
pub const CLOCK_REALTIME_COARSE: i32 = 5;
pub const CLOCK_MONOTONIC_COARSE: i32 = 6;
pub const CLOCK_BOOTTIME: i32 = 7;
pub const CLOCK_REALTIME_ALARM: i32 = 8;
pub const CLOCK_BOOTTIME_ALARM: i32 = 9;
pub const CLOCK_TAI: i32 = 11;

/// Retrieves the time of the specified clock.
pub fn sys_clock_gettime(clock_id: i32, tp_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    if tp_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let elapsed = ostd::timer::Jiffies::elapsed().as_duration();

    let ts = match clock_id {
        CLOCK_REALTIME | CLOCK_REALTIME_COARSE | CLOCK_REALTIME_ALARM | CLOCK_TAI => {
            let boot_epoch = get_boot_epoch_secs();
            let total_sec = boot_epoch + elapsed.as_secs();
            Timespec {
                tv_sec: total_sec as i64,
                tv_nsec: elapsed.subsec_nanos() as i64,
            }
        }
        CLOCK_MONOTONIC
        | CLOCK_MONOTONIC_RAW
        | CLOCK_MONOTONIC_COARSE
        | CLOCK_BOOTTIME
        | CLOCK_BOOTTIME_ALARM => Timespec {
            tv_sec: elapsed.as_secs() as i64,
            tv_nsec: elapsed.subsec_nanos() as i64,
        },
        CLOCK_PROCESS_CPUTIME_ID => {
            let td = Thread::current().ok_or(Errno::ESRCH)?;
            let exec_ns = crate::sched::stats::for_task(&td.td_kstack)
                .map(|s| s.exec_ns)
                .unwrap_or(0);
            Timespec {
                tv_sec: (exec_ns / 1_000_000_000) as i64,
                tv_nsec: (exec_ns % 1_000_000_000) as i64,
            }
        }
        CLOCK_THREAD_CPUTIME_ID => {
            let td = Thread::current().ok_or(Errno::ESRCH)?;
            let exec_ns = crate::sched::stats::for_task(&td.td_kstack)
                .map(|s| s.exec_ns)
                .unwrap_or(0);
            Timespec {
                tv_sec: (exec_ns / 1_000_000_000) as i64,
                tv_nsec: (exec_ns % 1_000_000_000) as i64,
            }
        }
        _ => return Err(Errno::EINVAL),
    };

    UserPtr::new(tp_ptr).write_bytes(&vmar, &ts.to_bytes())?;
    Ok(0)
}
