//! Safe User-Space C-String Abstraction for PetraOS.
//!
//! Provides `UserCStr` for safe and bounded reading of null-terminated C strings
//! from user-space memory into kernel space.

use alloc::ffi::CString;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::mm::VirtAddr;
use crate::mm::user::USER_SPACE_MAX_ADDR;
use crate::syscalls::SyscallError;

/// A typed wrapper around a user-space null-terminated C string pointer.
///
/// Encapsulates bounded traversal and verification of user-space memory
/// to prevent invalid memory reads and kernel faults.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct UserCStr {
    addr: VirtAddr,
}

impl UserCStr {
    /// Create a new `UserCStr` from a strongly-typed `VirtAddr`.
    #[inline(always)]
    pub const fn new(addr: VirtAddr) -> Self {
        Self { addr }
    }

    /// Create a `UserCStr` from a raw 64-bit virtual address integer.
    #[inline(always)]
    pub const fn from_u64(addr: u64) -> Self {
        Self::new(VirtAddr::new(addr))
    }

    /// Create a `UserCStr` from an unsigned byte raw pointer.
    #[inline(always)]
    pub fn from_ptr(ptr: *const u8) -> Self {
        Self::new(VirtAddr::new(ptr as u64))
    }

    /// Create a `UserCStr` from a signed byte (C `char`) raw pointer.
    #[inline(always)]
    pub fn from_i8_ptr(ptr: *const i8) -> Self {
        Self::new(VirtAddr::new(ptr as u64))
    }

    /// Create a `UserCStr` from a raw pointer (alias for `from_ptr`).
    #[inline(always)]
    pub fn from_raw(ptr: *const u8) -> Self {
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

    /// Returns the const raw byte pointer representation.
    #[inline(always)]
    pub const fn as_ptr(&self) -> *const u8 {
        self.addr.as_ptr()
    }

    /// Returns true if the pointer is null (address is zero).
    #[inline(always)]
    pub const fn is_null(&self) -> bool {
        self.addr.is_null()
    }

    /// Checks if the pointer is non-null and resides within user canonical address bounds.
    #[inline(always)]
    pub fn is_valid(&self) -> bool {
        !self.is_null() && self.addr.as_u64() <= USER_SPACE_MAX_ADDR
    }

    /// Computes the length of the string (excluding the null terminator) up to `max_len`.
    ///
    /// Returns `None` if the pointer is null, unmapped/invalid, or no null terminator
    /// is encountered within `max_len` bytes.
    #[must_use]
    pub fn len(&self, max_len: usize) -> Option<usize> {
        if !self.is_valid() {
            return None;
        }

        let base = self.addr.as_u64();
        for i in 0..max_len {
            let curr = base.checked_add(i as u64)?;
            if curr > USER_SPACE_MAX_ADDR {
                return None;
            }

            // SAFETY: Validated within canonical user-space address range.
            let byte = unsafe { core::ptr::read_volatile(curr as *const u8) };
            if byte == 0 {
                return Some(i);
            }
        }

        None
    }

    /// Reads the null-terminated byte sequence into an allocated `Vec<u8>`.
    ///
    /// Returns `None` if the string is invalid, unbounded, or exceeds `max_len`.
    #[must_use]
    pub fn as_vec(&self, max_len: usize) -> Option<Vec<u8>> {
        if !self.is_valid() {
            return None;
        }

        let base = self.addr.as_u64();
        let mut vec = Vec::new();

        for i in 0..max_len {
            let curr = base.checked_add(i as u64)?;
            if curr > USER_SPACE_MAX_ADDR {
                return None;
            }

            // SAFETY: Validated within canonical user-space address range.
            let byte = unsafe { core::ptr::read_volatile(curr as *const u8) };
            if byte == 0 {
                return Some(vec);
            }
            vec.push(byte);
        }

        None
    }

    /// Reads and converts the null-terminated string into a UTF-8 `String`.
    ///
    /// Returns `None` if the string exceeds `max_len`, faults, or contains invalid UTF-8.
    #[must_use]
    pub fn as_string(&self, max_len: usize) -> Option<String> {
        let bytes = self.as_vec(max_len)?;
        String::from_utf8(bytes).ok()
    }

    /// Reads and converts the string into an owned `CString`.
    #[must_use]
    pub fn as_cstring(&self, max_len: usize) -> Option<CString> {
        let bytes = self.as_vec(max_len)?;
        CString::new(bytes).ok()
    }

    /// Converts the user C string into a kernel `String` with POSIX `SyscallError` mapping.
    ///
    /// - Returns `Err(SyscallError::EFAULT)` if the pointer is null, unbounded, or faults.
    /// - Returns `Err(SyscallError::EINVAL)` if the string content is not valid UTF-8.
    pub fn to_string(&self, max_len: usize) -> Result<String, SyscallError> {
        let bytes = self.as_vec(max_len).ok_or(SyscallError::EFAULT)?;
        String::from_utf8(bytes).map_err(|_| SyscallError::EINVAL)
    }
}

impl From<VirtAddr> for UserCStr {
    #[inline(always)]
    fn from(addr: VirtAddr) -> Self {
        Self::new(addr)
    }
}

impl From<u64> for UserCStr {
    #[inline(always)]
    fn from(addr: u64) -> Self {
        Self::from_u64(addr)
    }
}

impl From<*const u8> for UserCStr {
    #[inline(always)]
    fn from(ptr: *const u8) -> Self {
        Self::from_ptr(ptr)
    }
}

impl From<*const i8> for UserCStr {
    #[inline(always)]
    fn from(ptr: *const i8) -> Self {
        Self::from_i8_ptr(ptr)
    }
}

impl fmt::Debug for UserCStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            f.write_str("UserCStr(<null>)")
        } else if let Some(s) = self.as_string(64) {
            write!(f, "UserCStr({:?} @ {:#018x})", s, self.addr.as_u64())
        } else {
            write!(f, "UserCStr(<fault> @ {:#018x})", self.addr.as_u64())
        }
    }
}

impl fmt::Display for UserCStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#018x}", self.addr.as_u64())
    }
}
