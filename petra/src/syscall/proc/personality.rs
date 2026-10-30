// SPDX-License-Identifier: GPL-2.0

//! `personality(2)` system call (Syscall #135 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::thread::Thread,
    uapi::errno::Result,
};

/// Default Linux execution domain.
pub const PER_LINUX: u32 = 0x0000;
/// Sentinel query value to inspect personality without changing it.
pub const PER_QUERY: usize = 0xffff_ffff;

/// System call `personality(unsigned long persona)` (Syscall #135).
pub fn sys_personality(persona: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let mut inner = proc.inner.lock();

    let old_personality = inner.personality;
    if persona != PER_QUERY && persona != usize::MAX {
        inner.personality = persona as u32;
    }

    Ok(old_personality as usize)
}
