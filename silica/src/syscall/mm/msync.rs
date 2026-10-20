// SPDX-License-Identifier: GPL-2.0

//! `msync(2)` system call.

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    vm::flags::MsyncFlags,
};

/// System call `msync(addr, len, flags)` (Syscall #26 on x86_64).
pub fn sys_msync(addr: usize, len: usize, flags_val: u32, _ctx: &mut UserContext) -> Result<usize> {
    let flags = MsyncFlags::from_raw(flags_val)?;
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    proc.vmspace().msync(flags, addr, len)?;
    Ok(0)
}
