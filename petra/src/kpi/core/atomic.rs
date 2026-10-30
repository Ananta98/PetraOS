// SPDX-License-Identifier: GPL-2.0

//! Linux atomic operations and bitops (`<linux/atomic.h>`, `<linux/bitops.h>`).
//!
//! Provides Linux-compatible `atomic_t`, `atomic64_t`, and atomic bit manipulation
//! primitives backed by `core::sync::atomic`.

pub use core::sync::atomic::{
    AtomicBool, AtomicI32, AtomicI64, AtomicIsize, AtomicU32, AtomicU64, AtomicUsize, Ordering,
};

/// Linux `atomic_t`: 32-bit signed atomic integer.
#[repr(transparent)]
#[derive(Debug, Default)]
pub struct AtomicI32Wrap(AtomicI32);

#[allow(non_camel_case_types)]
pub type atomic_t = AtomicI32Wrap;

impl AtomicI32Wrap {
    /// Creates a new `atomic_t` with the given initial value.
    pub const fn new(val: i32) -> Self {
        Self(AtomicI32::new(val))
    }

    /// Linux `atomic_read`: relaxed load.
    #[inline]
    pub fn read(&self) -> i32 {
        self.0.load(Ordering::Relaxed)
    }

    /// Linux `atomic_read_acquire`: acquire load.
    #[inline]
    pub fn read_acquire(&self) -> i32 {
        self.0.load(Ordering::Acquire)
    }

    /// Linux `atomic_set`: relaxed store.
    #[inline]
    pub fn set(&self, val: i32) {
        self.0.store(val, Ordering::Relaxed);
    }

    /// Linux `atomic_set_release`: release store.
    #[inline]
    pub fn set_release(&self, val: i32) {
        self.0.store(val, Ordering::Release);
    }

    /// Linux `atomic_add`: adds `val` without returning old value.
    #[inline]
    pub fn add(&self, val: i32) {
        self.0.fetch_add(val, Ordering::Relaxed);
    }

    /// Linux `atomic_sub`: subtracts `val` without returning old value.
    #[inline]
    pub fn sub(&self, val: i32) {
        self.0.fetch_sub(val, Ordering::Relaxed);
    }

    /// Linux `atomic_inc`: increments by 1.
    #[inline]
    pub fn inc(&self) {
        self.add(1);
    }

    /// Linux `atomic_dec`: decrements by 1.
    #[inline]
    pub fn dec(&self) {
        self.sub(1);
    }

    /// Linux `atomic_inc_and_test`: increments and returns `true` if the result is 0.
    #[inline]
    pub fn inc_and_test(&self) -> bool {
        self.add_return(1) == 0
    }

    /// Linux `atomic_dec_and_test`: decrements and returns `true` if the result is 0.
    #[inline]
    pub fn dec_and_test(&self) -> bool {
        self.sub_return(1) == 0
    }

    /// Linux `atomic_add_return`: adds `val` and returns new value with full barrier.
    #[inline]
    pub fn add_return(&self, val: i32) -> i32 {
        self.0.fetch_add(val, Ordering::SeqCst) + val
    }

    /// Linux `atomic_sub_return`: subtracts `val` and returns new value with full barrier.
    #[inline]
    pub fn sub_return(&self, val: i32) -> i32 {
        self.0.fetch_sub(val, Ordering::SeqCst) - val
    }

    /// Linux `atomic_xchg`: atomically swaps value and returns old value.
    #[inline]
    pub fn xchg(&self, val: i32) -> i32 {
        self.0.swap(val, Ordering::SeqCst)
    }

    /// Linux `atomic_cmpxchg`: compares with `old`, sets to `new` if matching, returns prior value.
    #[inline]
    pub fn cmpxchg(&self, old: i32, new: i32) -> i32 {
        match self.0.compare_exchange(old, new, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(v) => v,
            Err(v) => v,
        }
    }

    /// Linux `atomic_fetch_add`: adds and returns prior value.
    #[inline]
    pub fn fetch_add(&self, val: i32) -> i32 {
        self.0.fetch_add(val, Ordering::SeqCst)
    }

    /// Linux `atomic_fetch_sub`: subtracts and returns prior value.
    #[inline]
    pub fn fetch_sub(&self, val: i32) -> i32 {
        self.0.fetch_sub(val, Ordering::SeqCst)
    }
}

/// Linux `atomic64_t`: 64-bit signed atomic integer.
#[repr(transparent)]
#[derive(Debug, Default)]
pub struct AtomicI64Wrap(AtomicI64);

#[allow(non_camel_case_types)]
pub type atomic64_t = AtomicI64Wrap;

impl AtomicI64Wrap {
    /// Creates a new `atomic64_t` with the given initial value.
    pub const fn new(val: i64) -> Self {
        Self(AtomicI64::new(val))
    }

    /// Linux `atomic64_read`: relaxed load.
    #[inline]
    pub fn read(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }

    /// Linux `atomic64_set`: relaxed store.
    #[inline]
    pub fn set(&self, val: i64) {
        self.0.store(val, Ordering::Relaxed);
    }

    /// Linux `atomic64_add`: adds `val`.
    #[inline]
    pub fn add(&self, val: i64) {
        self.0.fetch_add(val, Ordering::Relaxed);
    }

    /// Linux `atomic64_sub`: subtracts `val`.
    #[inline]
    pub fn sub(&self, val: i64) {
        self.0.fetch_sub(val, Ordering::Relaxed);
    }

    /// Linux `atomic64_inc`: increments by 1.
    #[inline]
    pub fn inc(&self) {
        self.add(1);
    }

    /// Linux `atomic64_dec`: decrements by 1.
    #[inline]
    pub fn dec(&self) {
        self.sub(1);
    }

    /// Linux `atomic64_inc_and_test`: increments and returns `true` if the result is 0.
    #[inline]
    pub fn inc_and_test(&self) -> bool {
        self.add_return(1) == 0
    }

    /// Linux `atomic64_dec_and_test`: decrements and returns `true` if the result is 0.
    #[inline]
    pub fn dec_and_test(&self) -> bool {
        self.sub_return(1) == 0
    }

    /// Linux `atomic64_add_return`: adds `val` and returns new value with full barrier.
    #[inline]
    pub fn add_return(&self, val: i64) -> i64 {
        self.0.fetch_add(val, Ordering::SeqCst) + val
    }

    /// Linux `atomic64_sub_return`: subtracts `val` and returns new value with full barrier.
    #[inline]
    pub fn sub_return(&self, val: i64) -> i64 {
        self.0.fetch_sub(val, Ordering::SeqCst) - val
    }

    /// Linux `atomic64_xchg`: atomically swaps value and returns old value.
    #[inline]
    pub fn xchg(&self, val: i64) -> i64 {
        self.0.swap(val, Ordering::SeqCst)
    }

    /// Linux `atomic64_cmpxchg`: compares with `old`, sets to `new` if matching, returns prior value.
    #[inline]
    pub fn cmpxchg(&self, old: i64, new: i64) -> i64 {
        match self.0.compare_exchange(old, new, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(v) => v,
            Err(v) => v,
        }
    }
}

// ---------------------------------------------------------------------------
// Linux Bit Operations (`<linux/bitops.h>`)
// ---------------------------------------------------------------------------

const BITS_PER_USIZE: usize = core::mem::size_of::<usize>() * 8;

/// Linux `set_bit`: atomically sets bit `nr` in atomic word.
#[inline]
pub fn set_bit(nr: usize, addr: &AtomicUsize) {
    let mask = 1usize << (nr % BITS_PER_USIZE);
    addr.fetch_or(mask, Ordering::SeqCst);
}

/// Linux `clear_bit`: atomically clears bit `nr` in atomic word.
#[inline]
pub fn clear_bit(nr: usize, addr: &AtomicUsize) {
    let mask = 1usize << (nr % BITS_PER_USIZE);
    addr.fetch_and(!mask, Ordering::SeqCst);
}

/// Linux `change_bit`: atomically toggles bit `nr` in atomic word.
#[inline]
pub fn change_bit(nr: usize, addr: &AtomicUsize) {
    let mask = 1usize << (nr % BITS_PER_USIZE);
    addr.fetch_xor(mask, Ordering::SeqCst);
}

/// Linux `test_bit`: tests whether bit `nr` is set.
#[inline]
pub fn test_bit(nr: usize, addr: &AtomicUsize) -> bool {
    let mask = 1usize << (nr % BITS_PER_USIZE);
    (addr.load(Ordering::Relaxed) & mask) != 0
}

/// Linux `test_and_set_bit`: atomically sets bit `nr` and returns its prior value.
#[inline]
pub fn test_and_set_bit(nr: usize, addr: &AtomicUsize) -> bool {
    let mask = 1usize << (nr % BITS_PER_USIZE);
    (addr.fetch_or(mask, Ordering::SeqCst) & mask) != 0
}

/// Linux `test_and_clear_bit`: atomically clears bit `nr` and returns its prior value.
#[inline]
pub fn test_and_clear_bit(nr: usize, addr: &AtomicUsize) -> bool {
    let mask = 1usize << (nr % BITS_PER_USIZE);
    (addr.fetch_and(!mask, Ordering::SeqCst) & mask) != 0
}

/// Linux `test_and_change_bit`: atomically toggles bit `nr` and returns its prior value.
#[inline]
pub fn test_and_change_bit(nr: usize, addr: &AtomicUsize) -> bool {
    let mask = 1usize << (nr % BITS_PER_USIZE);
    (addr.fetch_xor(mask, Ordering::SeqCst) & mask) != 0
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn atomic_i32_operations() {
        let a = atomic_t::new(10);
        assert_eq!(a.read(), 10);
        a.set(20);
        assert_eq!(a.read(), 20);

        a.inc();
        assert_eq!(a.read(), 21);
        a.dec();
        assert_eq!(a.read(), 20);

        assert_eq!(a.add_return(5), 25);
        assert_eq!(a.sub_return(10), 15);

        assert_eq!(a.xchg(50), 15);
        assert_eq!(a.read(), 50);

        assert_eq!(a.cmpxchg(50, 100), 50);
        assert_eq!(a.read(), 100);

        let zero = atomic_t::new(-1);
        assert!(zero.inc_and_test());
        assert!(!zero.inc_and_test());
        assert!(zero.dec_and_test());
    }

    #[ktest]
    fn atomic_bit_operations() {
        let flags = AtomicUsize::new(0);
        assert!(!test_bit(3, &flags));

        set_bit(3, &flags);
        assert!(test_bit(3, &flags));

        assert!(test_and_clear_bit(3, &flags));
        assert!(!test_bit(3, &flags));

        assert!(!test_and_set_bit(5, &flags));
        assert!(test_bit(5, &flags));

        change_bit(5, &flags);
        assert!(!test_bit(5, &flags));
    }
}
