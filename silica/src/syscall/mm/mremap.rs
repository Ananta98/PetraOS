// SPDX-License-Identifier: GPL-2.0

//! `mremap(2)` system call.

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::Result,
    proc::thread::Thread,
    vm::flags::MremapFlags,
};

/// System call `mremap(old_address, old_size, new_size, flags, new_address)` (Syscall #25 on x86_64).
pub fn sys_mremap(
    old_addr: usize,
    old_size: usize,
    new_size: usize,
    flags_val: u32,
    new_addr_raw: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let flags = MremapFlags::from_raw(flags_val)?;
    let new_addr = if flags.contains(MremapFlags::FIXED) {
        Some(new_addr_raw)
    } else {
        None
    };

    let proc = Thread::proc_or_esrch()?;
    let addr = proc
        .vmspace()
        .mremap(old_addr, old_size, new_size, flags, new_addr)?;
    Ok(addr)
}
