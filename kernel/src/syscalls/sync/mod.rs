//! Synchronization System Calls
//!
//! Provides the POSIX `sys_futex` system call (Syscall #202 on x86_64) for userspace
//! fast synchronization primitives (mutexes, condition variables, semaphores, barriers).

use super::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::{PageTable, VirtAddr};
use crate::proc::thread::ThreadState;
use crate::sync::futex::{
    FutexKey, FUTEX_BITSET_MATCH_ANY, FUTEX_CLOCK_REALTIME, FUTEX_CMD_MASK, FUTEX_CMP_REQUEUE,
    FUTEX_CMP_REQUEUE_PI, FUTEX_FD, FUTEX_LOCK_PI, FUTEX_MANAGER, FUTEX_PRIVATE_FLAG,
    FUTEX_REQUEUE, FUTEX_TRYLOCK_PI, FUTEX_UNLOCK_PI, FUTEX_WAIT, FUTEX_WAIT_BITSET,
    FUTEX_WAIT_REQUEUE_PI, FUTEX_WAKE, FUTEX_WAKE_BITSET, FUTEX_WAKE_OP,
};

// ── Modular syscall submodules ──────────────────────────────────────────
pub mod futex;

pub use futex::sys_futex;

/// POSIX `struct timespec` for 64-bit architecture
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TimeSpec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

/// Helper to validate a user futex address (must be valid Ring 3 pointer and 4-byte aligned).
#[inline]
pub(crate) fn validate_futex_ptr(ptr: UserPtr<u32>) -> Result<(), SyscallError> {
    let addr = ptr.as_u64();
    if addr == 0 || (addr % 4) != 0 {
        return Err(SyscallError::EINVAL);
    }
    if !ptr.is_valid() {
        return Err(SyscallError::EFAULT);
    }
    Ok(())
}

/// Helper to resolve a `FutexKey` from a user-space virtual address.
pub(crate) fn resolve_futex_key(
    uaddr: UserPtr<u32>,
    is_private: bool,
    proc: &crate::proc::Process,
) -> FutexKey {
    let vaddr = uaddr.as_u64();
    if is_private {
        FutexKey::Private {
            pid: proc.pid.as_u64(),
            vaddr,
        }
    } else {
        let addr_space = proc.address_space.lock();
        if let Some(paddr) = addr_space.page_table().translate(VirtAddr::new(vaddr)) {
            FutexKey::Shared {
                paddr: paddr.as_u64() + (vaddr & 0xFFF),
            }
        } else {
            // Fallback to process-private key if physical translation is not yet established
            FutexKey::Private {
                pid: proc.pid.as_u64(),
                vaddr,
            }
        }
    }
}

/// Helper to safely parse a user-space `timespec` structure.
pub(crate) fn parse_user_timespec(timeout_ptr: UserPtr<TimeSpec>) -> Result<Option<TimeSpec>, SyscallError> {
    if timeout_ptr.is_null() {
        return Ok(None);
    }

    let ts = timeout_ptr.read().ok_or(SyscallError::EFAULT)?;
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
        return Err(SyscallError::EINVAL);
    }

    Ok(Some(ts))
}
