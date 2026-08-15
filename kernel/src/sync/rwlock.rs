//! Reader-Writer Lock Primitive (RWLock)
//!
//! Provides a concurrent synchronization primitive allowing multiple readers
//! or a single exclusive writer in a `#![no_std]` environment.

use core::cell::UnsafeCell;
use core::fmt;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicUsize, Ordering};

const WRITER_BIT: usize = 1 << (usize::BITS - 1);
const READER_MASK: usize = !WRITER_BIT;

/// A reader-writer lock.
///
/// This type of lock allows multiple readers or at most one writer at any point in time.
/// The write portion of this lock typically allows modification of the underlying data
/// (exclusive access) and the read portion of this lock allows read-only access
/// (shared access).
pub struct RwLock<T: ?Sized> {
    state: AtomicUsize,
    data: UnsafeCell<T>,
}

// Type aliases for casing flexibility matching both Rust conventions and RWLock naming.
pub type RWLock<T> = RwLock<T>;
pub type RWLockReadGuard<'a, T> = RwLockReadGuard<'a, T>;
pub type RWLockWriteGuard<'a, T> = RwLockWriteGuard<'a, T>;

// SAFETY: RwLock can be transferred across threads if the underlying type is `Send`.
unsafe impl<T: ?Sized + Send> Send for RwLock<T> {}
// SAFETY: RwLock enables concurrent shared reads and exclusive writes, requiring `Send + Sync`.
unsafe impl<T: ?Sized + Send + Sync> Sync for RwLock<T> {}

/// An RAII guard providing shared read-only access to the data protected by a [`RwLock`].
///
/// When this guard is dropped, the reader count is decremented.
pub struct RwLockReadGuard<'a, T: ?Sized> {
    lock: &'a RwLock<T>,
}

// SAFETY: `RwLockReadGuard` represents shared immutable access, which is thread-safe if `T: Sync`.
unsafe impl<T: ?Sized + Sync> Sync for RwLockReadGuard<'_, T> {}

/// An RAII guard providing exclusive mutable access to the data protected by a [`RwLock`].
///
/// When this guard is dropped, the exclusive write lock is released.
pub struct RwLockWriteGuard<'a, T: ?Sized> {
    lock: &'a RwLock<T>,
}

// SAFETY: `RwLockWriteGuard` represents exclusive access to the underlying data.
unsafe impl<T: ?Sized + Sync> Sync for RwLockWriteGuard<'_, T> {}

impl<T> RwLock<T> {
    /// Creates a new unlocked reader-writer lock.
    pub const fn new(data: T) -> Self {
        Self {
            state: AtomicUsize::new(0),
            data: UnsafeCell::new(data),
        }
    }

    /// Consumes the reader-writer lock, returning the underlying data.
    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }
}

impl<T: ?Sized> RwLock<T> {
    /// Locks this `RwLock` with shared read access, spinning until acquired.
    ///
    /// Multiple threads can hold shared read access concurrently as long as
    /// no thread holds exclusive write access.
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        loop {
            let count = self.state.load(Ordering::Relaxed);
            if count & WRITER_BIT != 0 || count == READER_MASK {
                core::hint::spin_loop();
                continue;
            }
            if self
                .state
                .compare_exchange_weak(count, count + 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return RwLockReadGuard { lock: self };
            }
            core::hint::spin_loop();
        }
    }

    /// Attempts to acquire shared read access without spinning.
    ///
    /// Returns `Some(RwLockReadGuard)` if acquired, or `None` if an exclusive write lock is held.
    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, T>> {
        let count = self.state.load(Ordering::Relaxed);
        if count & WRITER_BIT != 0 || count == READER_MASK {
            return None;
        }
        if self
            .state
            .compare_exchange(count, count + 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Some(RwLockReadGuard { lock: self })
        } else {
            None
        }
    }

    /// Locks this `RwLock` with exclusive write access, spinning until acquired.
    ///
    /// Only one thread can hold write access, and no threads may hold read access concurrently.
    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        while self
            .state
            .compare_exchange_weak(0, WRITER_BIT, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        RwLockWriteGuard { lock: self }
    }

    /// Attempts to acquire exclusive write access without spinning.
    ///
    /// Returns `Some(RwLockWriteGuard)` if acquired, or `None` if any readers or writers exist.
    pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T>> {
        if self
            .state
            .compare_exchange(0, WRITER_BIT, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Some(RwLockWriteGuard { lock: self })
        } else {
            None
        }
    }

    /// Returns `true` if the lock is held in either read or write mode.
    pub fn is_locked(&self) -> bool {
        self.state.load(Ordering::Relaxed) != 0
    }

    /// Returns `true` if the lock is currently held by an exclusive writer.
    pub fn is_write_locked(&self) -> bool {
        self.state.load(Ordering::Relaxed) & WRITER_BIT != 0
    }

    /// Returns the number of active shared readers.
    pub fn reader_count(&self) -> usize {
        self.state.load(Ordering::Relaxed) & READER_MASK
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// Since this requires a mutable reference to `self`, no locking is necessary.
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }

    /// Forcibly resets the lock to an unlocked state.
    ///
    /// # Safety
    /// This is unsafe because it circumvents read and write invariants.
    /// It should only be used in emergency paths (such as kernel panic recovery).
    pub unsafe fn force_unlock(&self) {
        self.state.store(0, Ordering::Release);
    }
}

impl<T: ?Sized> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: The reader lock is held, ensuring no concurrent mutable writers.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T: ?Sized> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.state.fetch_sub(1, Ordering::Release);
    }
}

impl<T: ?Sized> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: Exclusive write lock is held, ensuring unique mutable access.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T: ?Sized> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: Exclusive write lock is held, ensuring unique mutable access.
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T: ?Sized> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.state.store(0, Ordering::Release);
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLock<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("RwLock");
        if let Some(guard) = self.try_read() {
            d.field("data", &&*guard);
        } else {
            d.field("data", &format_args!("<locked>"));
        }
        d.finish()
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLockReadGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for RwLockReadGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLockWriteGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for RwLockWriteGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: Default> Default for RwLock<T> {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<T> From<T> for RwLock<T> {
    fn from(data: T) -> Self {
        Self::new(data)
    }
}
