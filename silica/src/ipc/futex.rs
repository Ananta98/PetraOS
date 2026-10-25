// SPDX-License-Identifier: GPL-2.0

//! Fast userspace mutex (`futex(2)`) implementation using `hashbrown::HashMap`.
//!
//! Follows Linux x86-64 semantics for futex operations, matching the Linux ABI
//! and syscall number 202.

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use ostd::sync::Waker;
use spin::{Lazy, Mutex};

use crate::{
    uapi::errno::{Errno, Result},
    utils::UserPtr,
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
pub static FUTEX_TABLE: Lazy<Mutex<BTreeMap<FutexKey, Vec<FutexWaiter>>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

/// Removes a specific waiter from the futex hash table.
pub fn remove_waiter(key: &FutexKey, woken: &Arc<AtomicBool>) {
    let mut table = FUTEX_TABLE.lock();
    if let Some(waiters) = table.get_mut(key) {
        waiters.retain(|w| !Arc::ptr_eq(&w.woken, woken));
        if waiters.is_empty() {
            table.remove(key);
        }
    }
}

/// Wakes up to `val` waiters matching `bitset` for the specified `key`.
pub fn wake_waiters(key: &FutexKey, val: u32, bitset: u32) -> usize {
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
pub fn read_futex_val(vmar: &Arc<Vmar>, uaddr: usize) -> Result<u32> {
    let mut bytes = [0u8; 4];
    UserPtr::new(uaddr).read_bytes(vmar, &mut bytes)?;
    Ok(u32::from_ne_bytes(bytes))
}

/// Reads a Linux `struct timespec` from user space at `utime_ptr`.
pub fn read_timespec(vmar: &Arc<Vmar>, utime_ptr: usize) -> Result<(i64, i64)> {
    let mut bytes = [0u8; 16];
    UserPtr::new(utime_ptr).read_bytes(vmar, &mut bytes)?;
    let sec = i64::from_ne_bytes(bytes[0..8].try_into().unwrap());
    let nsec = i64::from_ne_bytes(bytes[8..16].try_into().unwrap());
    if sec < 0 || !(0..1_000_000_000).contains(&nsec) {
        return Err(Errno::EINVAL);
    }
    Ok((sec, nsec))
}
