//! sys_futex system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::proc::thread::ThreadState;
use crate::sync::futex::{
    FUTEX_BITSET_MATCH_ANY, FUTEX_CLOCK_REALTIME, FUTEX_CMD_MASK, FUTEX_CMP_REQUEUE,
    FUTEX_CMP_REQUEUE_PI, FUTEX_FD, FUTEX_LOCK_PI, FUTEX_MANAGER, FUTEX_PRIVATE_FLAG,
    FUTEX_REQUEUE, FUTEX_TRYLOCK_PI, FUTEX_UNLOCK_PI, FUTEX_WAIT, FUTEX_WAIT_BITSET,
    FUTEX_WAIT_REQUEUE_PI, FUTEX_WAKE, FUTEX_WAKE_BITSET, FUTEX_WAKE_OP,
};


pub fn sys_futex(frame: &mut SyscallFrame) -> SyscallResult {
    let uaddr = UserPtr::<u32>::from_u64(frame.arg1());
    let futex_op = frame.arg2() as u32;
    let val = frame.arg3() as u32;
    let timeout_or_val2 = frame.arg4();
    let uaddr2 = UserPtr::<u32>::from_u64(frame.arg5());
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
            let timeout_ptr = UserPtr::<TimeSpec>::from_u64(timeout_or_val2);
            let timeout = parse_user_timespec(timeout_ptr)?;

            // Convert relative timeout to absolute HPET nanoseconds
            let deadline_ns = timeout.map(|ts| {
                let dur_ns = (ts.tv_sec as u64)
                    .saturating_mul(1_000_000_000)
                    .saturating_add(ts.tv_nsec as u64);
                if is_realtime {
                    // For CLOCK_REALTIME, compute duration relative to CMOS RTC wall clock
                    let (now_sec, now_usec) = crate::drivers::time::cmos_rtc::get_wall_time();
                    let now_wall_ns = now_sec
                        .saturating_mul(1_000_000_000)
                        .saturating_add(now_usec.saturating_mul(1_000));
                    let remaining = dur_ns.saturating_sub(now_wall_ns);
                    crate::arch::timer::hpet::elapsed_ns().saturating_add(remaining)
                } else {
                    crate::arch::timer::hpet::elapsed_ns().saturating_add(dur_ns)
                }
            });

            // Atomically verify futex word and enqueue thread into wait queue
            {
                let mut mgr = FUTEX_MANAGER.lock();
                // SAFETY: `uaddr` is verified and valid for 4-byte read.
                unsafe {
                    mgr.wait_prepare(
                        key,
                        thread_arc.clone(),
                        uaddr.as_ptr(),
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
            let (woken, requeued) =
                mgr.requeue(key, key2, wake_count, requeue_count, FUTEX_BITSET_MATCH_ANY);
            Ok(woken + requeued)
        }

        FUTEX_CMP_REQUEUE => {
            validate_futex_ptr(uaddr2)?;
            let proc = proc_arc.lock();
            let key2 = resolve_futex_key(uaddr2, is_private, &proc);
            drop(proc);

            let current_val = uaddr.read().ok_or(SyscallError::EFAULT)?;
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
            let timeout_ptr = UserPtr::<TimeSpec>::from_u64(timeout_or_val2);
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
                    mgr.wait_prepare(key, thread_arc.clone(), uaddr.as_ptr(), val, bitset, deadline_ns)?;
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
            log::warn!("Unknown futex operation {}", futex_op);
            Err(SyscallError::EINVAL)
        }
    }
}
