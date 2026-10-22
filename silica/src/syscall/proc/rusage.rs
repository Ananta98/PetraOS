// SPDX-License-Identifier: GPL-2.0

//! `getrusage(2)` system call (Syscall #98 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::thread::Thread,
    uapi::{
        errno::{Errno, Result},
        limit::Rusage,
    },
    utils::userspace::user_ptr::UserPtr,
};

const RUSAGE_SELF: i32 = 0;
const RUSAGE_CHILDREN: i32 = -1;
const RUSAGE_THREAD: i32 = 1;

/// `getrusage(2)` - returns resource usage measures.
pub fn sys_getrusage(who: i32, usage_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    if who != RUSAGE_SELF && who != RUSAGE_CHILDREN && who != RUSAGE_THREAD {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::proc_or_esrch()?;
    let ru = match who {
        RUSAGE_CHILDREN => proc.inner.lock().children_rusage,
        RUSAGE_SELF | RUSAGE_THREAD => {
            let mut self_ru = Rusage::default();
            let mapped_bytes = proc.vmspace().total_mapped_size();
            self_ru.ru_maxrss = (mapped_bytes / 1024) as i64;
            self_ru
        }
        _ => return Err(Errno::EINVAL),
    };

    let buf = ru.to_bytes();
    UserPtr::new(usage_ptr).write_bytes(&proc.vmspace(), &buf)?;

    Ok(0)
}
