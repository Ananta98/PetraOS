// SPDX-License-Identifier: GPL-2.0

//! Time-of-day system calls:
//! - `gettimeofday(2)` (Syscall #96 on x86_64)
//! - `settimeofday(2)` (Syscall #164 on x86_64)

use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::Thread,
    time::Timeval,
    uapi::errno::{Errno, Result},
    utils::UserPtr,
};

/// Gets time of day and timezone (`gettimeofday(2)`, Syscall #96).
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

/// Sets time of day and timezone (`settimeofday(2)`, Syscall #164).
///
/// Reuses the realtime-offset machinery behind `clock_settime(CLOCK_REALTIME)`
/// so both syscalls shift the same clock. The timezone argument is validated
/// but discarded: the emulated clock always runs in UTC.
pub fn sys_settimeofday(tv_ptr: usize, tz_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    if tv_ptr == 0 && tz_ptr == 0 {
        return Ok(0);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    // Only root may change the system time (or the timezone).
    if !proc.cred().cr_uid.is_root() {
        return Err(Errno::EPERM);
    }

    if tv_ptr != 0 {
        let mut buf = [0u8; 16];
        UserPtr::new(tv_ptr).read_bytes(&vmar, &mut buf)?;
        let new_tv = Timeval::from_bytes(&buf).ok_or(Errno::EINVAL)?;

        if !new_tv.is_valid() {
            return Err(Errno::EINVAL);
        }

        let new_total_ns = new_tv.total_nanos().ok_or(Errno::EINVAL)?;
        let current_ns = crate::time::get_realtime_ns();
        let delta_ns = (new_total_ns as i128) - (current_ns as i128);

        crate::time::adjust_realtime_offset(delta_ns as i64);
    }

    if tz_ptr != 0 {
        // Read only to prove the pointer is valid; the value is not stored.
        let mut tz = [0u8; 8];
        UserPtr::new(tz_ptr).read_bytes(&vmar, &mut tz)?;
    }

    Ok(0)
}
