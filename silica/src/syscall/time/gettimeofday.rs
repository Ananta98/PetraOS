// SPDX-License-Identifier: GPL-2.0

//! `gettimeofday(2)` system call (Syscall #96 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::Result,
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
};

/// Gets time of day and timezone.
pub fn sys_gettimeofday(tv_ptr: usize, tz_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    if tv_ptr != 0 {
        let tv = crate::time::get_timeval();
        UserPtr::new(tv_ptr).write_bytes(&vmar, &tv.to_bytes())?;
    }

    if tz_ptr != 0 {
        // Obsolete timezone structure: minuteswest=0, dsttime=0
        let zero_tz = [0u8; 8];
        UserPtr::new(tz_ptr).write_bytes(&vmar, &zero_tz)?;
    }

    Ok(0)
}
