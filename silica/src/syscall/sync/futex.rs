// SPDX-License-Identifier: GPL-2.0

//! Fast userspace mutex (`futex(2)`) implementation using `hashbrown::HashMap`.
//!
//! Follows Linux x86-64 semantics for futex operations, matching the Linux ABI
//! and syscall number 202.

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use ostd::{
    arch::cpu::context::UserContext,
    sync::{Waiter, Waker},
    task::Task,
};
use spin::{Lazy, Mutex};

use crate::{
    uapi::errno::{Errno, Result},
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
    vm::vmar::Vmar,
};

/// Linux futex operation codes.
pub const FUTEX_WAIT: i32 = 0;
pub const FUTEX_WAKE: i32 = 1;
pub const FUTEX_FD: i32 = 2;
pub const FUTEX_REQUEUE: i32 = 3;
pub const FUTEX_CMP_REQUEUE: i32 = 4;
pub const FUTEX_WAKE_OP: i32 = 5;
pub const FUTEX_LOCK_PI: i32 = 6;
pub const FUTEX_UNLOCK_PI: i32 = 7;
pub const FUTEX_TRYLOCK_PI: i32 = 8;
pub const FUTEX_WAIT_BITSET: i32 = 9;
pub const FUTEX_WAKE_BITSET: i32 = 10;
pub const FUTEX_WAIT_REQUEUE_PI: i32 = 11;
pub const FUTEX_CMP_REQUEUE_PI: i32 = 12;

/// Linux futex flags.
pub const FUTEX_PRIVATE_FLAG: i32 = 128;
pub const FUTEX_CLOCK_REALTIME: i32 = 256;
pub const FUTEX_CMD_MASK: i32 = !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);

/// Default bitset matching any waiter.
pub const FUTEX_BITSET_MATCH_ANY: u32 = 0xffff_ffff;

/// A key identifying a futex word in the global hash table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FutexKey {
    /// Address space identifier: pointer to the Vmar for private futexes,
    /// or 0 for shared futexes across processes.
    pub space_id: usize,
    /// Virtual address of the 32-bit futex word in user space.
    pub uaddr: usize,
}

impl FutexKey {
    pub fn new(is_private: bool, vmar: &Arc<Vmar>, uaddr: usize) -> Self {
        let space_id = if is_private {
            Arc::as_ptr(vmar) as usize
        } else {
            0
        };
        Self { space_id, uaddr }
    }
}

/// A waiting thread registered in the futex hash table.
pub struct FutexWaiter {
    pub waker: Arc<Waker>,
    pub woken: Arc<AtomicBool>,
    pub bitset: u32,
}

/// Global futex hash table using `BTreeMap`.
static FUTEX_TABLE: Lazy<Mutex<BTreeMap<FutexKey, Vec<FutexWaiter>>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

/// Removes a specific waiter from the futex hash table.
fn remove_waiter(key: &FutexKey, woken: &Arc<AtomicBool>) {
    let mut table = FUTEX_TABLE.lock();
    if let Some(waiters) = table.get_mut(key) {
        waiters.retain(|w| !Arc::ptr_eq(&w.woken, woken));
        if waiters.is_empty() {
            table.remove(key);
        }
    }
}

/// Wakes up to `val` waiters matching `bitset` for the specified `key`.
fn wake_waiters(key: &FutexKey, val: u32, bitset: u32) -> usize {
    let mut table = FUTEX_TABLE.lock();
    let mut woken_count = 0;
    if let Some(waiters) = table.get_mut(key) {
        let mut i = 0;
        while i < waiters.len() && (woken_count as u32) < val {
            if (waiters[i].bitset & bitset) != 0 {
                let waiter = waiters.remove(i);
                waiter.woken.store(true, Ordering::Release);
                waiter.waker.wake_up();
                woken_count += 1;
            } else {
                i += 1;
            }
        }
        if waiters.is_empty() {
            table.remove(key);
        }
    }
    woken_count
}

/// Wakes waiters on a futex address (e.g. on thread exit for `clear_child_tid`).
pub fn futex_wake(vmar: &Arc<Vmar>, uaddr: usize, val: u32) -> usize {
    let key_priv = FutexKey::new(true, vmar, uaddr);
    let key_shared = FutexKey::new(false, vmar, uaddr);

    let mut count = wake_waiters(&key_priv, val, FUTEX_BITSET_MATCH_ANY);
    if (count as u32) < val {
        count += wake_waiters(&key_shared, val - count as u32, FUTEX_BITSET_MATCH_ANY);
    }
    count
}

/// Reads the 32-bit futex value from user space at `uaddr`.
fn read_futex_val(vmar: &Arc<Vmar>, uaddr: usize) -> Result<u32> {
    let mut bytes = [0u8; 4];
    UserPtr::new(uaddr).read_bytes(vmar, &mut bytes)?;
    Ok(u32::from_ne_bytes(bytes))
}

/// Reads a Linux `struct timespec` from user space at `utime_ptr`.
fn read_timespec(vmar: &Arc<Vmar>, utime_ptr: usize) -> Result<(i64, i64)> {
    let mut bytes = [0u8; 16];
    UserPtr::new(utime_ptr).read_bytes(vmar, &mut bytes)?;
    let sec = i64::from_ne_bytes(bytes[0..8].try_into().unwrap());
    let nsec = i64::from_ne_bytes(bytes[8..16].try_into().unwrap());
    if sec < 0 || !(0..1_000_000_000).contains(&nsec) {
        return Err(Errno::EINVAL);
    }
    Ok((sec, nsec))
}

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
            {
                let mut table = FUTEX_TABLE.lock();
                table.entry(key).or_default().push(FutexWaiter {
                    waker,
                    woken: woken.clone(),
                    bitset,
                });
            }

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
