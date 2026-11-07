// SPDX-License-Identifier: GPL-2.0

//! `rseq(2)` - Restartable sequences system call (Syscall #334 on x86_64).

use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;
use ostd::cpu::CpuId;
use ostd::user::UserContextApi;

use crate::{
    proc::{signal::QueuedSignal, thread::Thread, Proc},
    uapi::{
        errno::{Errno, Result},
        signal::{code, SigInfo, Signal},
    },
    utils::UserPtr,
};

/// Flag to unregister restartable sequence registration.
pub const RSEQ_FLAG_UNREGISTER: i32 = 1 << 0;
/// Minimum structure size for `struct rseq` according to Linux ABI.
pub const RSEQ_MIN_LEN: u32 = 32;
/// Maximum structure size for `struct rseq` according to Linux ABI (page size).
pub const RSEQ_MAX_LEN: u32 = 4096;

// Offsets in struct rseq according to Linux ABI
const RSEQ_OFF_CPU_ID_START: usize = 0;
const RSEQ_OFF_CPU_ID: usize = 4;
const RSEQ_OFF_CS: usize = 8;
const RSEQ_OFF_FLAGS: usize = 16;
const RSEQ_OFF_NODE_ID: usize = 20;
const RSEQ_OFF_MM_CID: usize = 24;

// Offsets in struct rseq_cs according to Linux ABI
const RSEQ_CS_OFF_VERSION: usize = 0;
const RSEQ_CS_OFF_FLAGS: usize = 4;
const RSEQ_CS_OFF_START_IP: usize = 8;
const RSEQ_CS_OFF_POST_COMMIT_OFFSET: usize = 16;
const RSEQ_CS_OFF_ABORT_IP: usize = 24;

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
        if inner.rseq_ptr == 0
            || inner.rseq_ptr != rseq_ptr
            || inner.rseq_len != rseq_len
            || inner.rseq_sig != sig
        {
            return Err(Errno::EINVAL);
        }
        inner.rseq_ptr = 0;
        inner.rseq_len = 0;
        inner.rseq_sig = 0;
        return Ok(0);
    }

    // Validate registration parameters
    if rseq_ptr == 0
        || !rseq_ptr.is_multiple_of(32)
        || rseq_len < RSEQ_MIN_LEN
        || rseq_len > RSEQ_MAX_LEN
    {
        return Err(Errno::EINVAL);
    }

    let proc = td.proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    {
        let inner = td.inner.lock();
        if inner.rseq_ptr != 0 {
            if inner.rseq_ptr == rseq_ptr && inner.rseq_len == rseq_len && inner.rseq_sig == sig {
                return Ok(0);
            }
            return Err(Errno::EBUSY);
        }
    }

    // Initialize cpu_id_start and cpu_id in the user struct with current CPU identifier
    let cpu_id = u32::from(CpuId::current_racy());
    let cpu_bytes = cpu_id.to_ne_bytes();
    UserPtr::new(rseq_ptr + RSEQ_OFF_CPU_ID_START).write_bytes(&vmar, &cpu_bytes)?;
    UserPtr::new(rseq_ptr + RSEQ_OFF_CPU_ID).write_bytes(&vmar, &cpu_bytes)?;

    if rseq_len >= 24 {
        let _ = UserPtr::new(rseq_ptr + RSEQ_OFF_NODE_ID).write_bytes(&vmar, &0u32.to_ne_bytes());
    }
    if rseq_len >= 28 {
        let _ = UserPtr::new(rseq_ptr + RSEQ_OFF_MM_CID).write_bytes(&vmar, &0u32.to_ne_bytes());
    }

    // Only commit registration to thread state once user memory writes succeed
    {
        let mut inner = td.inner.lock();
        inner.rseq_ptr = rseq_ptr;
        inner.rseq_len = rseq_len;
        inner.rseq_sig = sig;
    }

    Ok(0)
}

/// Delivers a SIGSEGV fault signal for invalid rseq memory access or signature mismatch.
fn send_rseq_sigsegv(td: &Thread, addr: usize) {
    let queued = QueuedSignal {
        info: SigInfo::fault(Signal::SIGSEGV, code::SEGV_MAPERR, addr),
    };
    td.deliver_fault_signal(queued);
}

/// Updates user-space `rseq` TLS state and checks for interrupted critical sections
/// before returning to userspace.
pub fn rseq_notify_resume(
    td: &Thread,
    proc: &Arc<Proc>,
    ctx: &mut UserContext,
    rseq_ptr: usize,
    _rseq_len: u32,
    rseq_sig: u32,
) {
    let vmar = proc.vmspace();

    // 1. Update cpu_id and cpu_id_start with current processor ID
    let cpu_id = u32::from(CpuId::current_racy());
    let cpu_bytes = cpu_id.to_ne_bytes();
    if UserPtr::new(rseq_ptr + RSEQ_OFF_CPU_ID_START)
        .write_bytes(&vmar, &cpu_bytes)
        .is_err()
        || UserPtr::new(rseq_ptr + RSEQ_OFF_CPU_ID)
            .write_bytes(&vmar, &cpu_bytes)
            .is_err()
    {
        send_rseq_sigsegv(td, rseq_ptr);
        return;
    }

    // 2. Read rseq_cs pointer from user memory
    let mut cs_ptr_buf = [0u8; 8];
    if UserPtr::new(rseq_ptr + RSEQ_OFF_CS)
        .read_bytes(&vmar, &mut cs_ptr_buf)
        .is_err()
    {
        send_rseq_sigsegv(td, rseq_ptr + RSEQ_OFF_CS);
        return;
    }
    let rseq_cs_ptr = usize::from_ne_bytes(cs_ptr_buf);
    if rseq_cs_ptr == 0 {
        return;
    }

    // Clear rseq_cs pointer in user TLS so it does not trigger again
    let zeros = [0u8; 8];
    if UserPtr::new(rseq_ptr + RSEQ_OFF_CS)
        .write_bytes(&vmar, &zeros)
        .is_err()
    {
        send_rseq_sigsegv(td, rseq_ptr + RSEQ_OFF_CS);
        return;
    }

    // 3. Validate struct rseq_cs address alignment
    if !rseq_cs_ptr.is_multiple_of(32) {
        send_rseq_sigsegv(td, rseq_cs_ptr);
        return;
    }

    // 4. Read struct rseq_cs
    let mut cs_buf = [0u8; 32];
    if UserPtr::new(rseq_cs_ptr)
        .read_bytes(&vmar, &mut cs_buf)
        .is_err()
    {
        send_rseq_sigsegv(td, rseq_cs_ptr);
        return;
    }

    let version = u32::from_ne_bytes(
        cs_buf[RSEQ_CS_OFF_VERSION..RSEQ_CS_OFF_VERSION + 4]
            .try_into()
            .unwrap(),
    );
    let start_ip = u64::from_ne_bytes(
        cs_buf[RSEQ_CS_OFF_START_IP..RSEQ_CS_OFF_START_IP + 8]
            .try_into()
            .unwrap(),
    );
    let post_commit_offset = u64::from_ne_bytes(
        cs_buf[RSEQ_CS_OFF_POST_COMMIT_OFFSET..RSEQ_CS_OFF_POST_COMMIT_OFFSET + 8]
            .try_into()
            .unwrap(),
    );
    let abort_ip = u64::from_ne_bytes(
        cs_buf[RSEQ_CS_OFF_ABORT_IP..RSEQ_CS_OFF_ABORT_IP + 8]
            .try_into()
            .unwrap(),
    );

    if version != 0 {
        send_rseq_sigsegv(td, rseq_cs_ptr);
        return;
    }

    // 5. Check if interrupted instruction pointer falls within critical section bounds
    let current_ip = ctx.instruction_pointer() as u64;
    let in_cs = current_ip >= start_ip && current_ip < start_ip.wrapping_add(post_commit_offset);

    if in_cs {
        // Interrupted inside critical section: verify safety signature at (abort_ip - 4)
        if abort_ip < 4 {
            send_rseq_sigsegv(td, abort_ip as usize);
            return;
        }

        let mut sig_buf = [0u8; 4];
        if UserPtr::new((abort_ip - 4) as usize)
            .read_bytes(&vmar, &mut sig_buf)
            .is_err()
        {
            send_rseq_sigsegv(td, abort_ip as usize);
            return;
        }

        let sig_found = u32::from_ne_bytes(sig_buf);
        if sig_found != rseq_sig {
            ostd::error!(
                "rseq: signature mismatch at {:#x}: expected {:#x}, found {:#x}",
                abort_ip - 4,
                rseq_sig,
                sig_found
            );
            send_rseq_sigsegv(td, abort_ip as usize);
            return;
        }

        // Restartable sequence aborted: redirect instruction pointer to user abort handler
        ctx.set_instruction_pointer(abort_ip as usize);
    }
}
