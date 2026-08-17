//! Synchronization System Calls
//!
//! Provides the POSIX `sys_futex` system call (Syscall #202 on x86_64) for userspace
//! fast synchronization primitives (mutexes, condition variables, semaphores, barriers).

use super::{is_user_ptr_valid, SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::PageTable;
use crate::proc::thread::ThreadState;
use x86_64::VirtAddr;
use crate::sync::futex::{
    FutexKey, FUTEX_BITSET_MATCH_ANY, FUTEX_CLOCK_REALTIME, FUTEX_CMD_MASK, FUTEX_CMP_REQUEUE,
    FUTEX_CMP_REQUEUE_PI, FUTEX_FD, FUTEX_LOCK_PI, FUTEX_MANAGER, FUTEX_PRIVATE_FLAG,
    FUTEX_REQUEUE, FUTEX_TRYLOCK_PI, FUTEX_UNLOCK_PI, FUTEX_WAIT, FUTEX_WAIT_BITSET,
    FUTEX_WAIT_REQUEUE_PI, FUTEX_WAKE, FUTEX_WAKE_BITSET, FUTEX_WAKE_OP,
};

/// POSIX `struct timespec` for 64-bit architecture
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TimeSpec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

/// Helper to validate a user futex address (must be valid Ring 3 pointer and 4-byte aligned).
#[inline]
fn validate_futex_ptr(ptr: *const u32) -> Result<(), SyscallError> {
    let addr = ptr as u64;
    if addr == 0 || (addr % 4) != 0 {
        return Err(SyscallError::EINVAL);
    }
    if !is_user_ptr_valid(addr, core::mem::size_of::<u32>()) {
        return Err(SyscallError::EFAULT);
    }
    Ok(())
}

/// Helper to resolve a `FutexKey` from a user-space virtual address.
fn resolve_futex_key(
    uaddr: *const u32,
    is_private: bool,
    proc: &crate::proc::Process,
) -> FutexKey {
    let vaddr = uaddr as u64;
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
fn parse_user_timespec(timeout_ptr: *const TimeSpec) -> Result<Option<TimeSpec>, SyscallError> {
    if timeout_ptr.is_null() {
        return Ok(None);
    }

    let addr = timeout_ptr as u64;
    if !is_user_ptr_valid(addr, core::mem::size_of::<TimeSpec>()) {
        return Err(SyscallError::EFAULT);
    }

    // SAFETY: Validated user memory within Ring 3 boundary.
    let ts = unsafe { core::ptr::read_volatile(timeout_ptr) };
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
        return Err(SyscallError::EINVAL);
    }

    Ok(Some(ts))
}

/// `sys_futex` (SYS_FUTEX = 202)
///
/// Fast Userspace Mutex system call providing wait, wake, and requeue primitives.
///
/// Arguments:
/// - `arg1` (uaddr): Pointer to 32-bit integer futex word in userspace.
/// - `arg2` (futex_op): Operation command and flags (`FUTEX_WAIT`, `FUTEX_WAKE`, etc.).
/// - `arg3` (val): Value expected at `uaddr` (for wait) or maximum waiters to wake.
/// - `arg4` (timeout / val2): Pointer to `timespec` (for wait) or number of waiters to requeue.
/// - `arg5` (uaddr2): Target pointer for requeue operations.
/// - `arg6` (val3): Expected value (for cmp_requeue) or bitset mask (for bitset wait/wake).
pub fn sys_futex(frame: &mut SyscallFrame) -> SyscallResult {
    let uaddr = frame.arg1() as *const u32;
    let futex_op = frame.arg2() as u32;
    let val = frame.arg3() as u32;
    let timeout_or_val2 = frame.arg4();
    let uaddr2 = frame.arg5() as *const u32;
    let val3 = frame.arg6() as u32;

    validate_futex_ptr(uaddr)?;

    let cmd = futex_op & FUTEX_CMD_MASK;
    let is_private = (futex_op & FUTEX_PRIVATE_FLAG) != 0;
    let is_realtime = (futex_op & FUTEX_CLOCK_REALTIME) != 0;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let key = resolve_futex_key(uaddr, is_private, &proc);
    drop(proc);

    match cmd {
        FUTEX_WAIT => {
            let thread_arc = crate::proc::current_thread().ok_or(SyscallError::ESRCH)?;
            let timeout_ptr = timeout_or_val2 as *const TimeSpec;
            let timeout = parse_user_timespec(timeout_ptr)?;

            let current_ns = crate::arch::timer::hpet::elapsed_ns();
            let deadline_ns = timeout.map(|ts| {
                let duration_ns = (ts.tv_sec as u64)
                    .saturating_mul(1_000_000_000)
                    .saturating_add(ts.tv_nsec as u64);
                if is_realtime {
                    duration_ns
                } else {
                    current_ns.saturating_add(duration_ns)
                }
            });

            // Enqueue thread in futex wait queue under lock
            {
                let mut mgr = FUTEX_MANAGER.lock();
                // SAFETY: `uaddr` is verified and valid for 4-byte read.
                unsafe {
                    mgr.wait_prepare(
                        key,
                        thread_arc.clone(),
                        uaddr,
                        val,
                        FUTEX_BITSET_MATCH_ANY,
                        deadline_ns,
                    )?;
                }
            }

            // Put current thread to sleep and switch context
            {
                let mut t = thread_arc.lock();
                t.state = ThreadState::Sleeping;
            }
            crate::sched::schedule(false);

            // Once unblocked, check if woken by timeout
            let tid = thread_arc.lock().tid;
            let now_ns = crate::arch::timer::hpet::elapsed_ns();
            if let Some(deadline) = deadline_ns {
                if now_ns >= deadline {
                    let mut mgr = FUTEX_MANAGER.lock();
                    if mgr.remove_waiter(&key, tid) {
                        return Err(SyscallError::ETIMEDOUT);
                    }
                }
            }

            Ok(0)
        }

        FUTEX_WAKE => {
            let count = val as usize;
            let mut mgr = FUTEX_MANAGER.lock();
            let woken = mgr.wake(key, count, FUTEX_BITSET_MATCH_ANY);
            Ok(woken)
        }

        FUTEX_REQUEUE => {
            validate_futex_ptr(uaddr2)?;
            let proc = proc_arc.lock();
            let key2 = resolve_futex_key(uaddr2, is_private, &proc);
            drop(proc);

            let wake_count = val as usize;
            let requeue_count = timeout_or_val2 as usize;

            let mut mgr = FUTEX_MANAGER.lock();
            let (woken, _) = mgr.requeue(key, key2, wake_count, requeue_count, FUTEX_BITSET_MATCH_ANY);
            Ok(woken)
        }

        FUTEX_CMP_REQUEUE => {
            validate_futex_ptr(uaddr2)?;
            let proc = proc_arc.lock();
            let key2 = resolve_futex_key(uaddr2, is_private, &proc);
            drop(proc);

            // SAFETY: `uaddr` validated with `validate_futex_ptr`.
            let current_val = unsafe { core::ptr::read_volatile(uaddr) };
            if current_val != val3 {
                return Err(SyscallError::EAGAIN);
            }

            let wake_count = val as usize;
            let requeue_count = timeout_or_val2 as usize;

            let mut mgr = FUTEX_MANAGER.lock();
            let (woken, requeued) =
                mgr.requeue(key, key2, wake_count, requeue_count, FUTEX_BITSET_MATCH_ANY);
            Ok(woken + requeued)
        }

        FUTEX_WAIT_BITSET => {
            let bitset = val3;
            if bitset == 0 {
                return Err(SyscallError::EINVAL);
            }

            let thread_arc = crate::proc::current_thread().ok_or(SyscallError::ESRCH)?;
            let timeout_ptr = timeout_or_val2 as *const TimeSpec;
            let timeout = parse_user_timespec(timeout_ptr)?;

            // In Linux, FUTEX_WAIT_BITSET timeouts are absolute timestamps
            let deadline_ns = timeout.map(|ts| {
                (ts.tv_sec as u64)
                    .saturating_mul(1_000_000_000)
                    .saturating_add(ts.tv_nsec as u64)
            });

            // Enqueue thread in futex wait queue under lock
            {
                let mut mgr = FUTEX_MANAGER.lock();
                // SAFETY: `uaddr` is verified and valid for 4-byte read.
                unsafe {
                    mgr.wait_prepare(key, thread_arc.clone(), uaddr, val, bitset, deadline_ns)?;
                }
            }

            // Put current thread to sleep and switch context
            {
                let mut t = thread_arc.lock();
                t.state = ThreadState::Sleeping;
            }
            crate::sched::schedule(false);

            // Once unblocked, check if woken by timeout
            let tid = thread_arc.lock().tid;
            let now_ns = crate::arch::timer::hpet::elapsed_ns();
            if let Some(deadline) = deadline_ns {
                if now_ns >= deadline {
                    let mut mgr = FUTEX_MANAGER.lock();
                    if mgr.remove_waiter(&key, tid) {
                        return Err(SyscallError::ETIMEDOUT);
                    }
                }
            }

            Ok(0)
        }

        FUTEX_WAKE_BITSET => {
            let bitset = val3;
            if bitset == 0 {
                return Err(SyscallError::EINVAL);
            }

            let count = val as usize;
            let mut mgr = FUTEX_MANAGER.lock();
            let woken = mgr.wake(key, count, bitset);
            Ok(woken)
        }

        FUTEX_FD
        | FUTEX_WAKE_OP
        | FUTEX_LOCK_PI
        | FUTEX_UNLOCK_PI
        | FUTEX_TRYLOCK_PI
        | FUTEX_WAIT_REQUEUE_PI
        | FUTEX_CMP_REQUEUE_PI => {
            log::warn!("Futex operation {} not currently supported", cmd);
            Err(SyscallError::ENOSYS)
        }

        _ => {
            log::warn!("Unknown futex operation: {}", futex_op);
            Err(SyscallError::EINVAL)
        }
    }
}
