//! Strongly-Typed User-Space Pointer Abstraction for PetraOS.
//!
//! Provides `UserPtr<T>` for safe, structured, and bounded access to user-space memory
//! from within kernel space without manual pointer arithmetic or unsafe dereferencing.

use core::fmt;
use core::marker::PhantomData;
use core::mem::size_of;

use crate::mm::VirtAddr;
use crate::mm::user::USER_SPACE_MAX_ADDR;

/// A typed wrapper around a user-space virtual memory address.
///
/// Ensures that memory accesses are restricted to canonical Ring 3 user-space address ranges.
#[repr(transparent)]
pub struct UserPtr<T: Sized + Copy> {
    addr: VirtAddr,
    _marker: PhantomData<T>,
}

impl<T: Sized + Copy> Clone for UserPtr<T> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Sized + Copy> Copy for UserPtr<T> {}

impl<T: Sized + Copy> PartialEq for UserPtr<T> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.addr == other.addr
    }
}

impl<T: Sized + Copy> Eq for UserPtr<T> {}

impl<T: Sized + Copy> PartialOrd for UserPtr<T> {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Sized + Copy> Ord for UserPtr<T> {
    #[inline(always)]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.addr.cmp(&other.addr)
    }
}

impl<T: Sized + Copy> core::hash::Hash for UserPtr<T> {
    #[inline(always)]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.addr.hash(state);
    }
}

impl<T: Sized + Copy> Default for UserPtr<T> {
    #[inline(always)]
    fn default() -> Self {
        Self {
            addr: VirtAddr::zero(),
            _marker: PhantomData,
        }
    }
}

impl<T: Sized + Copy> UserPtr<T> {
    /// Create a new `UserPtr` from a strongly-typed `VirtAddr`.
    #[inline(always)]
    pub const fn new(addr: VirtAddr) -> Self {
        Self {
            addr,
            _marker: PhantomData,
        }
    }

    /// Create a `UserPtr` from a raw 64-bit virtual address integer.
    #[inline(always)]
    pub const fn from_u64(addr: u64) -> Self {
        Self::new(VirtAddr::new(addr))
    }

    /// Create a `UserPtr` from a const raw pointer.
    #[inline(always)]
    pub fn from_ptr(ptr: *const T) -> Self {
        Self::new(VirtAddr::new(ptr as u64))
    }

    /// Create a `UserPtr` from a mutable raw pointer.
    #[inline(always)]
    pub fn from_mut_ptr(ptr: *mut T) -> Self {
        Self::new(VirtAddr::new(ptr as u64))
    }

    /// Create a `UserPtr` from a raw pointer (alias for `from_ptr`).
    #[inline(always)]
    pub fn from_raw(ptr: *const T) -> Self {
        Self::from_ptr(ptr)
    }

    /// Returns the underlying `VirtAddr`.
    #[inline(always)]
    pub const fn addr(&self) -> VirtAddr {
        self.addr
    }

    /// Returns the raw 64-bit integer virtual address.
    #[inline(always)]
    pub const fn as_u64(&self) -> u64 {
        self.addr.as_u64()
    }

    /// Returns the const raw pointer representation.
    #[inline(always)]
    pub const fn as_ptr(&self) -> *const T {
        self.addr.as_ptr()
    }

    /// Returns the mutable raw pointer representation.
    #[inline(always)]
    pub const fn as_mut_ptr(&self) -> *mut T {
        self.addr.as_mut_ptr()
    }

    /// Returns true if the pointer is null (address is zero).
    #[inline(always)]
    pub const fn is_null(&self) -> bool {
        self.addr.is_null()
    }

    /// Checks whether the user pointer and single element `T` reside strictly within
    /// valid Ring 3 canonical user address space.
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.is_valid_for(size_of::<T>())
    }

    /// Checks whether a buffer of `byte_len` starting from this pointer lies strictly within
    /// valid Ring 3 canonical user address space without overflow.
    #[inline]
    pub fn is_valid_for(&self, byte_len: usize) -> bool {
        if self.is_null() {
            return false;
        }
        match self.addr.as_u64().checked_add(byte_len as u64) {
            Some(end) => end <= USER_SPACE_MAX_ADDR,
            None => false,
        }
    }

    /// Casts this user pointer to another type `R`.
    #[inline(always)]
    pub const fn cast<R: Sized + Copy>(self) -> UserPtr<R> {
        UserPtr {
            addr: self.addr,
            _marker: PhantomData,
        }
    }

    /// Converts this user pointer to another type `R` (alias for `cast`).
    #[inline(always)]
    pub const fn convert<R: Sized + Copy>(self) -> UserPtr<R> {
        self.cast()
    }

    /// Advances the pointer by `count` elements of type `T`.
    #[inline]
    pub fn offset(self, count: usize) -> Self {
        let byte_offset = (count * size_of::<T>()) as u64;
        Self::new(self.addr + byte_offset)
    }

    /// Advances the pointer by `count` elements (alias for `offset`).
    #[inline(always)]
    pub fn add(self, count: usize) -> Self {
        self.offset(count)
    }

    /// Safely reads a value of type `T` from user space memory.
    ///
    /// Returns `None` if the pointer is null, invalid, or outside user space bounds.
    #[must_use]
    pub fn read(&self) -> Option<T> {
        if !self.is_valid() {
            return None;
        }

        // SAFETY: Pointer is validated to reside strictly in user-space canonical memory bounds.
        Some(unsafe { core::ptr::read_volatile(self.as_ptr()) })
    }

    /// Safely reads an unaligned value of type `T` from user space memory.
    ///
    /// Returns `None` if the pointer is null, invalid, or outside user space bounds.
    #[must_use]
    pub fn read_unaligned(&self) -> Option<T> {
        if !self.is_valid() {
            return None;
        }

        // SAFETY: Pointer is validated to reside strictly in user-space canonical memory bounds.
        Some(unsafe { core::ptr::read_unaligned(self.as_ptr()) })
    }

    /// Safely writes a value of type `T` to user space memory.
    ///
    /// Returns `None` if the pointer is null, invalid, or outside user space bounds.
    #[must_use]
    pub fn write(&self, value: T) -> Option<()> {
        if !self.is_valid() {
            return None;
        }

        // SAFETY: Pointer is validated to reside strictly in user-space canonical memory bounds.
        unsafe {
            core::ptr::write_volatile(self.as_mut_ptr(), value);
        }
        Some(())
    }

    /// Safely copies an array of elements from user space into the destination slice `buf`.
    ///
    /// Returns `None` if the source range exceeds user-space boundaries.
    #[must_use]
    pub fn read_slice(&self, buf: &mut [T]) -> Option<()> {
        let bytes = core::mem::size_of_val(buf);
        if bytes == 0 {
            return Some(());
        }
        if !self.is_valid_for(bytes) {
            return None;
        }

        // SAFETY: Destination buffer and source user memory range are validated and non-overlapping.
        unsafe {
            core::ptr::copy_nonoverlapping(self.as_ptr(), buf.as_mut_ptr(), buf.len());
        }
        Some(())
    }

    /// Safely copies an array of elements from slice `buf` into user space memory.
    ///
    /// Returns `None` if the destination range exceeds user-space boundaries.
    #[must_use]
    pub fn write_slice(&self, buf: &[T]) -> Option<()> {
        let bytes = core::mem::size_of_val(buf);
        if bytes == 0 {
            return Some(());
        }
        if !self.is_valid_for(bytes) {
            return None;
        }

        // SAFETY: Destination user memory range and source buffer are validated and non-overlapping.
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), self.as_mut_ptr(), buf.len());
        }
        Some(())
    }

    /// Borrows a contiguous slice of `count` elements from user memory.
    ///
    /// Returns `None` if the address range is invalid or out of user bounds.
    #[must_use]
    pub fn as_slice<'a>(&self, count: usize) -> Option<&'a [T]> {
        let bytes = count.checked_mul(size_of::<T>())?;
        if bytes == 0 {
            return Some(&[]);
        }
        if !self.is_valid_for(bytes) {
            return None;
        }

        // SAFETY: Memory region is non-null and validated within canonical user space bounds.
        Some(unsafe { core::slice::from_raw_parts(self.as_ptr(), count) })
    }

    /// Borrows a mutable contiguous slice of `count` elements from user memory.
    ///
    /// Returns `None` if the address range is invalid or out of user bounds.
    #[must_use]
    pub fn as_slice_mut<'a>(&self, count: usize) -> Option<&'a mut [T]> {
        let bytes = count.checked_mul(size_of::<T>())?;
        if bytes == 0 {
            return Some(&mut []);
        }
        if !self.is_valid_for(bytes) {
            return None;
        }

        // SAFETY: Memory region is non-null and validated within canonical user space bounds.
        Some(unsafe { core::slice::from_raw_parts_mut(self.as_mut_ptr(), count) })
    }
}

impl<T: Sized + Copy> From<VirtAddr> for UserPtr<T> {
    #[inline(always)]
    fn from(addr: VirtAddr) -> Self {
        Self::new(addr)
    }
}

impl<T: Sized + Copy> From<u64> for UserPtr<T> {
    #[inline(always)]
    fn from(addr: u64) -> Self {
        Self::from_u64(addr)
    }
}

impl<T: Sized + Copy> From<*const T> for UserPtr<T> {
    #[inline(always)]
    fn from(ptr: *const T) -> Self {
        Self::from_ptr(ptr)
    }
}

impl<T: Sized + Copy> From<*mut T> for UserPtr<T> {
    #[inline(always)]
    fn from(ptr: *mut T) -> Self {
        Self::from_mut_ptr(ptr)
    }
}

impl<T: Sized + Copy> fmt::Debug for UserPtr<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UserPtr<{}>({:#018x})", core::any::type_name::<T>(), self.addr.as_u64())
    }
}

impl<T: Sized + Copy> fmt::Display for UserPtr<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#018x}", self.addr.as_u64())
    }
}
