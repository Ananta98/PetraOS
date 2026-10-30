// SPDX-License-Identifier: GPL-2.0

//! `rseq(2)` - Restartable sequences system call (Syscall #334 on x86_64).

use ostd::arch::cpu::context::UserContext;
use ostd::cpu::CpuId;

use crate::{
    proc::thread::Thread,
    uapi::errno::{Errno, Result},
    utils::UserPtr,
};

/// Flag to unregister restartable sequence registration.
pub const RSEQ_FLAG_UNREGISTER: i32 = 1 << 0;
/// Minimum structure size for `struct rseq` according to Linux ABI.
pub const RSEQ_MIN_LEN: u32 = 32;

/// System call `rseq(struct rseq *rseq, uint32_t rseq_len, int flags, uint32_t sig)` (Syscall #334).
pub fn sys_rseq(
    rseq_ptr: usize,
    rseq_len: u32,
    flags: i32,
    sig: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if (flags & !RSEQ_FLAG_UNREGISTER) != 0 {
        return Err(Errno::EINVAL);
    }

    let td = Thread::current().ok_or(Errno::ESRCH)?;

    // Handle unregistration request
    if (flags & RSEQ_FLAG_UNREGISTER) != 0 {
        let mut inner = td.inner.lock();
        if inner.rseq_ptr != rseq_ptr || inner.rseq_sig != sig {
            return Err(Errno::EINVAL);
        }
        inner.rseq_ptr = 0;
        inner.rseq_len = 0;
        inner.rseq_sig = 0;
        return Ok(0);
    }

    // Validate registration parameters
    if rseq_ptr == 0 || !rseq_ptr.is_multiple_of(32) || rseq_len < RSEQ_MIN_LEN {
        return Err(Errno::EINVAL);
    }

    let proc = td.proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    {
        let mut inner = td.inner.lock();
        if inner.rseq_ptr != 0 {
            if inner.rseq_ptr == rseq_ptr && inner.rseq_len == rseq_len && inner.rseq_sig == sig {
                return Ok(0);
            }
            return Err(Errno::EBUSY);
        }

        inner.rseq_ptr = rseq_ptr;
        inner.rseq_len = rseq_len;
        inner.rseq_sig = sig;
    }

    // Initialize cpu_id_start and cpu_id in the user struct with current CPU identifier
    let cpu_id = u32::from(CpuId::current_racy());
    let cpu_bytes = cpu_id.to_ne_bytes();
    UserPtr::new(rseq_ptr).write_bytes(&vmar, &cpu_bytes)?;
    UserPtr::new(rseq_ptr + 4).write_bytes(&vmar, &cpu_bytes)?;

    Ok(0)
}
