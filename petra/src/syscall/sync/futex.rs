// SPDX-License-Identifier: GPL-2.0

//! Fast userspace mutex (`futex(2)`) syscall adapter.

use crate::proc::Thread;
use crate::{
    ipc::futex::{
        FUTEX_BITSET_MATCH_ANY, FUTEX_CLOCK_REALTIME, FUTEX_CMD_MASK, FUTEX_CMP_REQUEUE,
        FUTEX_PRIVATE_FLAG, FUTEX_REQUEUE, FUTEX_TABLE, FUTEX_WAIT, FUTEX_WAIT_BITSET, FUTEX_WAKE,
        FUTEX_WAKE_BITSET, FutexKey, FutexWaiter, read_futex_val, read_timespec, remove_waiter,
        wake_waiters,
    },
    uapi::errno::{Errno, Result},
};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use ostd::arch::cpu::context::UserContext;
use ostd::sync::Waiter;
use ostd::task::Task;

/// `futex(2)` system call (Syscall #202 on x86_64).
pub fn sys_futex(
    uaddr: usize,
    op: i32,
    val: u32,
    utime: usize,
    uaddr2: usize,
    val3: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    // Futex addresses must be 4-byte aligned
    if !uaddr.is_multiple_of(4) {
        return Err(Errno::EINVAL);
    }

    let cmd = op & FUTEX_CMD_MASK;
    let is_private = (op & FUTEX_PRIVATE_FLAG) != 0;
    let is_realtime = (op & FUTEX_CLOCK_REALTIME) != 0;

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    match cmd {
        FUTEX_WAIT | FUTEX_WAIT_BITSET => {
            let bitset = if cmd == FUTEX_WAIT_BITSET {
                if val3 == 0 {
                    return Err(Errno::EINVAL);
                }
                val3
            } else {
                FUTEX_BITSET_MATCH_ANY
            };

            // Parse timeout if provided
            let (has_timeout, is_absolute, timeout_ticks) = if utime != 0 {
                let (sec, nsec) = read_timespec(&vmar, utime)?;
                let ms = (sec as u64)
                    .saturating_mul(1000)
                    .saturating_add((nsec as u64) / 1_000_000);
                let ticks = (ms * crate::sched::TIMER_HZ).div_ceil(1000);
                let absolute = cmd == FUTEX_WAIT_BITSET || is_realtime;
                (true, absolute, ticks)
            } else {
                (false, false, 0)
            };

            let key = FutexKey::new(is_private, &vmar, uaddr);

            // Read current value and compare
            let cur = read_futex_val(&vmar, uaddr)?;
            if cur != val {
                return Err(Errno::EAGAIN);
            }

            // Create waiter and register in hash map
            let (waiter, waker) = Waiter::new_pair();
            let woken = Arc::new(AtomicBool::new(false));

            let mut table = FUTEX_TABLE.lock();
            table.entry(key).or_default().push(FutexWaiter {
                waker,
                woken: woken.clone(),
                bitset,
            });
            drop(table);

            // Wait until woken, timed out, or interrupted by signal
            let start_tick = crate::sched::now();
            let result = loop {
                if woken.load(Ordering::Acquire) {
                    break Ok(0);
                }

                if Thread::should_interrupt() {
                    remove_waiter(&key, &woken);
                    if woken.load(Ordering::Acquire) {
                        break Ok(0);
                    } else {
                        break Err(Errno::EINTR);
                    }
                }

                if has_timeout {
                    let now = crate::sched::now();
                    let expired = if is_absolute {
                        now >= timeout_ticks
                    } else {
                        now.saturating_sub(start_tick) >= timeout_ticks
                    };

                    if expired {
                        remove_waiter(&key, &woken);
                        if woken.load(Ordering::Acquire) {
                            break Ok(0);
                        } else {
                            break Err(Errno::ETIMEDOUT);
                        }
                    }
                }

                Task::yield_now();
            };

            drop(waiter);
            result
        }

        FUTEX_WAKE | FUTEX_WAKE_BITSET => {
            let bitset = if cmd == FUTEX_WAKE_BITSET {
                if val3 == 0 {
                    return Err(Errno::EINVAL);
                }
                val3
            } else {
                FUTEX_BITSET_MATCH_ANY
            };

            let key = FutexKey::new(is_private, &vmar, uaddr);
            let woken_count = wake_waiters(&key, val, bitset);
            Ok(woken_count)
        }

        FUTEX_REQUEUE => {
            if !uaddr2.is_multiple_of(4) {
                return Err(Errno::EINVAL);
            }

            let key1 = FutexKey::new(is_private, &vmar, uaddr);
            let key2 = FutexKey::new(is_private, &vmar, uaddr2);

            let val2 = utime as u32; // In requeue, utime argument is val2
            let mut table = FUTEX_TABLE.lock();
            let mut woken_count = 0;

            if let Some(mut waiters) = table.remove(&key1) {
                // Wake up to val waiters
                while !waiters.is_empty() && (woken_count as u32) < val {
                    let waiter = waiters.remove(0);
                    waiter.woken.store(true, Ordering::Release);
                    waiter.waker.wake_up();
                    woken_count += 1;
                }

                // Move up to val2 remaining waiters to key2
                let mut requeued_count = 0;
                let target = table.entry(key2).or_default();
                while !waiters.is_empty() && requeued_count < val2 {
                    target.push(waiters.remove(0));
                    requeued_count += 1;
                }

                // If any waiters still left on key1, put them back
                if !waiters.is_empty() {
                    table.insert(key1, waiters);
                }
            }

            Ok(woken_count)
        }

        FUTEX_CMP_REQUEUE => {
            if !uaddr2.is_multiple_of(4) {
                return Err(Errno::EINVAL);
            }

            // Check current value against val3
            let cur = read_futex_val(&vmar, uaddr)?;
            if cur != val3 {
                return Err(Errno::EAGAIN);
            }

            let key1 = FutexKey::new(is_private, &vmar, uaddr);
            let key2 = FutexKey::new(is_private, &vmar, uaddr2);

            let val2 = utime as u32; // In cmp_requeue, utime is val2
            let mut table = FUTEX_TABLE.lock();
            let mut woken_count = 0;

            if let Some(mut waiters) = table.remove(&key1) {
                // Wake up to val waiters
                while !waiters.is_empty() && (woken_count as u32) < val {
                    let waiter = waiters.remove(0);
                    waiter.woken.store(true, Ordering::Release);
                    waiter.waker.wake_up();
                    woken_count += 1;
                }

                // Move up to val2 remaining waiters to key2
                let mut requeued_count = 0;
                let target = table.entry(key2).or_default();
                while !waiters.is_empty() && requeued_count < val2 {
                    target.push(waiters.remove(0));
                    requeued_count += 1;
                }

                // If any waiters still left on key1, put them back
                if !waiters.is_empty() {
                    table.insert(key1, waiters);
                }
            }

            Ok(woken_count)
        }

        _ => {
            ostd::warn!("Unsupported futex operation: op={op:#x}, cmd={cmd}");
            Err(Errno::ENOSYS)
        }
    }
}
