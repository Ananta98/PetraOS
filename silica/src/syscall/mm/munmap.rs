// SPDX-License-Identifier: GPL-2.0

use crate::{
    api::errno::Result,
    proc::thread::Thread,
};
use ostd::arch::cpu::context::UserContext;

pub fn sys_munmap(addr: usize, len: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    proc.vmspace().munmap(addr..addr + len)?;
    Ok(0)
}
