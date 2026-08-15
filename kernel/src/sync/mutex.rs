//! Mutual Exclusion Primitive (Mutex)
//!
//! Provides mutual exclusion for protecting shared kernel resources in a
//! `#![no_std]` environment.

use core::cell::UnsafeCell;
use core::fmt;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

/// A mutual exclusion primitive useful for protecting shared data.
///
/// This mutex provides interior mutability and ensures that only one thread/core
/// can access the guarded data at any given time.
pub struct Mutex<T: ?Sized> {
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

// SAFETY: Mutex controls concurrent access, allowing safe transfer across threads if `T` is `Send`.
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
// SAFETY: Mutex synchronizes access, enabling shared references across threads if `T` is `Send`.
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

/// An RAII guard providing scoped exclusive access to the data protected by a [`Mutex`].
///
/// When this guard is dropped, the mutex is automatically released.
pub struct MutexGuard<'a, T: ?Sized> {
    lock: &'a Mutex<T>,
}

// SAFETY: A `MutexGuard` represents exclusive access to the underlying data.
unsafe impl<T: ?Sized + Sync> Sync for MutexGuard<'_, T> {}

impl<T> Mutex<T> {
    /// Creates a new unlocked mutex in an unlocked state.
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    /// Consumes the mutex, returning the underlying data.
    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }
}

impl<T: ?Sized> Mutex<T> {
    /// Acquires the mutex, spinning until the lock becomes available.
    ///
    /// Returns an RAII [`MutexGuard`] that grants exclusive mutable access
    /// to the protected data until dropped.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        while self
            .lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        MutexGuard { lock: self }
    }

    /// Attempts to acquire the mutex without spinning.
    ///
    /// Returns `Some(MutexGuard)` if acquired, or `None` if the mutex is currently locked.
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        if self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Some(MutexGuard { lock: self })
        } else {
            None
        }
    }

    /// Returns `true` if the mutex is currently locked.
    pub fn is_locked(&self) -> bool {
        self.lock.load(Ordering::Relaxed)
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// Since this requires a mutable reference to `self`, no locking is necessary.
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }

    /// Forcibly unlocks the mutex.
    ///
    /// # Safety
    /// This function is unsafe because it drops exclusive access invariants.
    /// It should only be used in emergency paths (such as kernel panic recovery or post-crash unwinding).
    pub unsafe fn force_unlock(&self) {
        self.lock.store(false, Ordering::Release);
    }
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: The lock is held exclusively by this guard, guaranteeing no concurrent writers or readers.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: The lock is held exclusively by this guard, guaranteeing unique mutable access.
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.lock.store(false, Ordering::Release);
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("Mutex");
        if let Some(guard) = self.try_lock() {
            d.field("data", &&*guard);
        } else {
            d.field("data", &format_args!("<locked>"));
        }
        d.finish()
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<T> From<T> for Mutex<T> {
    fn from(data: T) -> Self {
        Self::new(data)
    }
}
