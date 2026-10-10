// SPDX-License-Identifier: GPL-2.0

//! `getrusage(2)` system call (Syscall #98 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
};

const RUSAGE_SELF: i32 = 0;
const RUSAGE_CHILDREN: i32 = -1;
const RUSAGE_THREAD: i32 = 1;

/// Linux x86_64 compatible `struct rusage` (144 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Rusage {
    pub ru_utime_sec: i64,
    pub ru_utime_usec: i64,
    pub ru_stime_sec: i64,
    pub ru_stime_usec: i64,
    pub ru_maxrss: i64,
    pub ru_ixrss: i64,
    pub ru_idrss: i64,
    pub ru_isrss: i64,
    pub ru_minflt: i64,
    pub ru_majflt: i64,
    pub ru_nswap: i64,
    pub ru_inblock: i64,
    pub ru_oublock: i64,
    pub ru_msgsnd: i64,
    pub ru_msgrcv: i64,
    pub ru_nsignals: i64,
    pub ru_nvcsw: i64,
    pub ru_nivcsw: i64,
}

impl Rusage {
    pub fn to_bytes(&self) -> [u8; 144] {
        let mut buf = [0u8; 144];
        buf[0..8].copy_from_slice(&self.ru_utime_sec.to_ne_bytes());
        buf[8..16].copy_from_slice(&self.ru_utime_usec.to_ne_bytes());
        buf[16..24].copy_from_slice(&self.ru_stime_sec.to_ne_bytes());
        buf[24..32].copy_from_slice(&self.ru_stime_usec.to_ne_bytes());
        buf[32..40].copy_from_slice(&self.ru_maxrss.to_ne_bytes());
        buf[40..48].copy_from_slice(&self.ru_ixrss.to_ne_bytes());
        buf[48..56].copy_from_slice(&self.ru_idrss.to_ne_bytes());
        buf[56..64].copy_from_slice(&self.ru_isrss.to_ne_bytes());
        buf[64..72].copy_from_slice(&self.ru_minflt.to_ne_bytes());
        buf[72..80].copy_from_slice(&self.ru_majflt.to_ne_bytes());
        buf[80..88].copy_from_slice(&self.ru_nswap.to_ne_bytes());
        buf[88..96].copy_from_slice(&self.ru_inblock.to_ne_bytes());
        buf[96..104].copy_from_slice(&self.ru_oublock.to_ne_bytes());
        buf[104..112].copy_from_slice(&self.ru_msgsnd.to_ne_bytes());
        buf[112..120].copy_from_slice(&self.ru_msgrcv.to_ne_bytes());
        buf[120..128].copy_from_slice(&self.ru_nsignals.to_ne_bytes());
        buf[128..136].copy_from_slice(&self.ru_nvcsw.to_ne_bytes());
        buf[136..144].copy_from_slice(&self.ru_nivcsw.to_ne_bytes());
        buf
    }
}

/// `getrusage(2)` - returns resource usage measures.
pub fn sys_getrusage(who: i32, usage_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    if who != RUSAGE_SELF && who != RUSAGE_CHILDREN && who != RUSAGE_THREAD {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let mut ru = Rusage::default();

    // Approximate memory footprint from mapped virtual address space
    let mapped_bytes = proc.vmspace().total_mapped_size();
    ru.ru_maxrss = (mapped_bytes / 1024) as i64;

    let buf = ru.to_bytes();
    UserPtr::new(usage_ptr).write_bytes(&proc.vmspace(), &buf)?;

    Ok(0)
}
