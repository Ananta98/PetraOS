// SPDX-License-Identifier: GPL-2.0

//! `getcpu` system call.

use ostd::arch::cpu::context::UserContext;
use ostd::cpu::CpuId;

use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
    utils::userspace::UserPtr,
};

/// `getcpu(2)` - Determines CPU and NUMA node on which the calling thread is running.
pub fn sys_getcpu(
    cpu_ptr: usize,
    node_ptr: usize,
    _tcache_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    if cpu_ptr != 0 {
        let cpu_id = u32::from(CpuId::current_racy());
        UserPtr::new(cpu_ptr).write_bytes(&vmar, &cpu_id.to_ne_bytes())?;
    }

    if node_ptr != 0 {
        let node_id: u32 = 0;
        UserPtr::new(node_ptr).write_bytes(&vmar, &node_id.to_ne_bytes())?;
    }

    Ok(0)
}
