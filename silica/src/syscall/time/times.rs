// SPDX-License-Identifier: GPL-2.0

//! `times(2)` system call (Syscall #100 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
};

/// Linux x86_64 compatible `struct tms` (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tms {
    pub tms_utime: i64,
    pub tms_stime: i64,
    pub tms_cutime: i64,
    pub tms_cstime: i64,
}

impl Tms {
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf[0..8].copy_from_slice(&self.tms_utime.to_ne_bytes());
        buf[8..16].copy_from_slice(&self.tms_stime.to_ne_bytes());
        buf[16..24].copy_from_slice(&self.tms_cutime.to_ne_bytes());
        buf[24..32].copy_from_slice(&self.tms_cstime.to_ne_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 32 {
            return None;
        }
        let tms_utime = i64::from_ne_bytes(buf[0..8].try_into().ok()?);
        let tms_stime = i64::from_ne_bytes(buf[8..16].try_into().ok()?);
        let tms_cutime = i64::from_ne_bytes(buf[16..24].try_into().ok()?);
        let tms_cstime = i64::from_ne_bytes(buf[24..32].try_into().ok()?);
        Some(Self {
            tms_utime,
            tms_stime,
            tms_cutime,
            tms_cstime,
        })
    }
}

/// Retrieves process execution times and returns elapsed system ticks (`times(2)`, Syscall #100).
pub fn sys_times(buf_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    if buf_ptr != 0 {
        let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
        let vmar = proc.vmspace();

        let td = Thread::current().ok_or(Errno::ESRCH)?;
        let exec_ns = crate::sched::stats::for_task(&td.td_kstack)
            .map(|s| s.exec_ns)
            .unwrap_or(0);

        // Convert to clock ticks at USER_HZ (100 Hz): 1 tick = 10_000_000 ns
        let utime_ticks = ((exec_ns as u128 * 100) / 1_000_000_000) as i64;

        let tms = Tms {
            tms_utime: utime_ticks,
            tms_stime: 0,
            tms_cutime: 0,
            tms_cstime: 0,
        };

        UserPtr::new(buf_ptr).write_bytes(&vmar, &tms.to_bytes())?;
    }

    // Returns the number of clock ticks elapsed since system boot in USER_HZ (100 Hz)
    let elapsed_ticks = crate::time::get_uptime_ticks(100) as usize;
    Ok(elapsed_ticks)
}
