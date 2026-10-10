// SPDX-License-Identifier: GPL-2.0

//! `gettimeofday(2)` system call (Syscall #96 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    syscall::time::{get_boot_epoch_secs, Timeval},
    utils::userspace::user_ptr::UserPtr,
};

/// Gets time of day and timezone.
pub fn sys_gettimeofday(tv_ptr: usize, tz_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    if tv_ptr != 0 {
        let elapsed = ostd::timer::Jiffies::elapsed().as_duration();
        let boot_epoch = get_boot_epoch_secs();
        let sec = (boot_epoch + elapsed.as_secs()) as i64;
        let usec = elapsed.subsec_micros() as i64;

        let tv = Timeval {
            tv_sec: sec,
            tv_usec: usec,
        };
        UserPtr::new(tv_ptr).write_bytes(&vmar, &tv.to_bytes())?;
    }

    if tz_ptr != 0 {
        // Obsolete timezone structure: minuteswest=0, dsttime=0
        let zero_tz = [0u8; 8];
        UserPtr::new(tz_ptr).write_bytes(&vmar, &zero_tz)?;
    }

    Ok(0)
}
