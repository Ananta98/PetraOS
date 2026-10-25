// SPDX-License-Identifier: GPL-2.0

//! System calls for eventfd: `eventfd`, `eventfd2`.

use ostd::arch::cpu::context::UserContext;

use crate::{
    fs::{create_eventfd_file, fd::FdFlags},
    proc::thread::Thread,
    uapi::{
        errno::{Errno, Result},
        fd::{EFD_CLOEXEC, EFD_VALID_FLAGS_MASK},
    },
};

/// System call `eventfd(unsigned int initval)` (Syscall #284).
pub fn sys_eventfd(initval: u32, ctx: &mut UserContext) -> Result<usize> {
    sys_eventfd2(initval, 0, ctx)
}

/// System call `eventfd2(unsigned int initval, int flags)` (Syscall #290).
pub fn sys_eventfd2(initval: u32, flags: i32, _ctx: &mut UserContext) -> Result<usize> {
    if (flags & !EFD_VALID_FLAGS_MASK) != 0 {
        return Err(Errno::EINVAL);
    }

    let file = create_eventfd_file(initval, flags);
    let mut descriptor_flags = FdFlags::empty();
    if (flags & EFD_CLOEXEC) != 0 {
        descriptor_flags |= FdFlags::FD_CLOEXEC;
    }

    let proc = Thread::proc_or_esrch()?;
    let new_fd = proc.fd_table.lock().alloc_fd(file, descriptor_flags)?;
    Ok(new_fd as usize)
}
