// SPDX-License-Identifier: GPL-2.0

//! System calls `mlock(2)`, `munlock(2)`, `mlockall(2)`, `munlockall(2)`, and `mlock2(2)`
//! (Syscalls #149, #150, #151, #152, and #325 on x86_64).

use ostd::{arch::cpu::context::UserContext, mm::PAGE_SIZE};

use crate::{
    proc::thread::Thread,
    uapi::errno::{Errno, Result},
};

const MCL_CURRENT: i32 = 1;
const MCL_FUTURE: i32 = 2;
const MCL_ONFAULT: i32 = 4;
const MLOCK_ONFAULT: i32 = 1;

fn round_range(addr: usize, len: usize) -> Result<(usize, usize)> {
    if len == 0 {
        return Err(Errno::EINVAL);
    }
    let aligned_addr = addr & !(PAGE_SIZE - 1);
    let end = addr.checked_add(len).ok_or(Errno::EINVAL)?;
    let aligned_end = end.checked_next_multiple_of(PAGE_SIZE).ok_or(Errno::EINVAL)?;
    Ok((aligned_addr, aligned_end))
}

/// System call `mlock(const void *addr, size_t len)` (Syscall #149).
pub fn sys_mlock(addr: usize, len: usize, _ctx: &mut UserContext) -> Result<usize> {
    let (start, end) = round_range(addr, len)?;
    let proc = Thread::proc_or_esrch()?;
    let _ = proc.vmspace().mlock_range(start..end, true);
    Ok(0)
}

/// System call `munlock(const void *addr, size_t len)` (Syscall #150).
pub fn sys_munlock(addr: usize, len: usize, _ctx: &mut UserContext) -> Result<usize> {
    let (start, end) = round_range(addr, len)?;
    let proc = Thread::proc_or_esrch()?;
    let _ = proc.vmspace().mlock_range(start..end, false);
    Ok(0)
}

/// System call `mlockall(int flags)` (Syscall #151).
pub fn sys_mlockall(flags: i32, _ctx: &mut UserContext) -> Result<usize> {
    if flags == 0 || (flags & !(MCL_CURRENT | MCL_FUTURE | MCL_ONFAULT)) != 0 {
        return Err(Errno::EINVAL);
    }
    Ok(0)
}

/// System call `munlockall(void)` (Syscall #152).
pub fn sys_munlockall(_ctx: &mut UserContext) -> Result<usize> {
    Ok(0)
}

/// System call `mlock2(const void *addr, size_t len, int flags)` (Syscall #325).
pub fn sys_mlock2(addr: usize, len: usize, flags: i32, _ctx: &mut UserContext) -> Result<usize> {
    if (flags & !MLOCK_ONFAULT) != 0 {
        return Err(Errno::EINVAL);
    }
    let (start, end) = round_range(addr, len)?;
    let proc = Thread::proc_or_esrch()?;
    let _ = proc.vmspace().mlock_range(start..end, true);
    Ok(0)
}
