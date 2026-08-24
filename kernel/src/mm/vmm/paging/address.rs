//! Strongly-typed Physical and Virtual Address Abstractions for PetraOS.
//!
//! Provides `PhysAddr` and `VirtAddr` with alignment, indexing, and pointer conversion helpers.

use core::fmt;
use core::ops::{Add, AddAssign, Sub, SubAssign};

/// A 64-bit physical memory address.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct PhysAddr(u64);

impl PhysAddr {
    /// Creates a new physical address.
    #[inline(always)]
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    /// Creates a zero physical address.
    #[inline(always)]
    pub const fn zero() -> Self {
        Self(0)
    }

    /// Returns the raw 64-bit integer address value.
    #[inline(always)]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Converts the physical address to a raw pointer.
    #[inline(always)]
    pub const fn as_ptr<T>(self) -> *const T {
        self.0 as *const T
    }

    /// Converts the physical address to a mutable raw pointer.
    #[inline(always)]
    pub const fn as_mut_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }

    /// Returns true if the address is aligned to the given boundary.
    #[inline(always)]
    pub const fn is_aligned(self, align: u64) -> bool {
        (self.0 & (align - 1)) == 0
    }

    /// Aligns the address downwards to the given boundary.
    #[inline(always)]
    pub const fn align_down(self, align: u64) -> Self {
        Self(self.0 & !(align - 1))
    }

    /// Aligns the address upwards to the given boundary.
    #[inline(always)]
    pub const fn align_up(self, align: u64) -> Self {
        Self((self.0 + align - 1) & !(align - 1))
    }

    /// Returns true if the address is null/zero.
    #[inline(always)]
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }
}

impl Add<u64> for PhysAddr {
    type Output = Self;
    #[inline(always)]
    fn add(self, rhs: u64) -> Self {
        Self(self.0 + rhs)
    }
}

impl AddAssign<u64> for PhysAddr {
    #[inline(always)]
    fn add_assign(&mut self, rhs: u64) {
        self.0 += rhs;
    }
}

impl Sub<u64> for PhysAddr {
    type Output = Self;
    #[inline(always)]
    fn sub(self, rhs: u64) -> Self {
        Self(self.0 - rhs)
    }
}

impl Sub<PhysAddr> for PhysAddr {
    type Output = u64;
    #[inline(always)]
    fn sub(self, rhs: PhysAddr) -> u64 {
        self.0 - rhs.0
    }
}

impl SubAssign<u64> for PhysAddr {
    #[inline(always)]
    fn sub_assign(&mut self, rhs: u64) {
        self.0 -= rhs;
    }
}

impl fmt::Debug for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PhysAddr({:#018x})", self.0)
    }
}

impl fmt::Display for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#018x}", self.0)
    }
}

impl fmt::LowerHex for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0, f)
    }
}

/// A 64-bit canonical virtual memory address.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct VirtAddr(u64);

impl VirtAddr {
    /// Creates a new virtual address.
    #[inline(always)]
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    /// Creates a zero virtual address.
    #[inline(always)]
    pub const fn zero() -> Self {
        Self(0)
    }

    /// Returns the raw 64-bit integer address value.
    #[inline(always)]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Converts the virtual address to a raw pointer.
    #[inline(always)]
    pub const fn as_ptr<T>(self) -> *const T {
        self.0 as *const T
    }

    /// Converts the virtual address to a mutable raw pointer.
    #[inline(always)]
    pub const fn as_mut_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }

    /// Returns true if the address is aligned to the given boundary.
    #[inline(always)]
    pub const fn is_aligned(self, align: u64) -> bool {
        (self.0 & (align - 1)) == 0
    }

    /// Aligns the address downwards to the given boundary.
    #[inline(always)]
    pub const fn align_down(self, align: u64) -> Self {
        Self(self.0 & !(align - 1))
    }

    /// Aligns the address upwards to the given boundary.
    #[inline(always)]
    pub const fn align_up(self, align: u64) -> Self {
        Self((self.0 + align - 1) & !(align - 1))
    }

    /// Returns true if the address is null/zero.
    #[inline(always)]
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }

    /// Returns the 9-bit Level 5 (PML5) page table index (bits 48..56).
    #[inline(always)]
    pub const fn pml5_index(self) -> usize {
        ((self.0 >> 48) & 0x1FF) as usize
    }

    /// Returns the 9-bit Level 4 (PML4) page table index (bits 39..47).
    #[inline(always)]
    pub const fn pml4_index(self) -> usize {
        ((self.0 >> 39) & 0x1FF) as usize
    }

    /// Returns the 9-bit Level 3 (PDPT) page table index (bits 30..38).
    #[inline(always)]
    pub const fn pdpt_index(self) -> usize {
        ((self.0 >> 30) & 0x1FF) as usize
    }

    /// Returns the 9-bit Level 2 (PD) page table index (bits 21..29).
    #[inline(always)]
    pub const fn pd_index(self) -> usize {
        ((self.0 >> 21) & 0x1FF) as usize
    }

    /// Returns the 9-bit Level 1 (PT) page table index (bits 12..20).
    #[inline(always)]
    pub const fn pt_index(self) -> usize {
        ((self.0 >> 12) & 0x1FF) as usize
    }

    /// Returns the 12-bit physical page offset (bits 0..11).
    #[inline(always)]
    pub const fn page_offset(self) -> u64 {
        self.0 & 0xFFF
    }
}

impl Add<u64> for VirtAddr {
    type Output = Self;
    #[inline(always)]
    fn add(self, rhs: u64) -> Self {
        Self(self.0 + rhs)
    }
}

impl AddAssign<u64> for VirtAddr {
    #[inline(always)]
    fn add_assign(&mut self, rhs: u64) {
        self.0 += rhs;
    }
}

impl Sub<u64> for VirtAddr {
    type Output = Self;
    #[inline(always)]
    fn sub(self, rhs: u64) -> Self {
        Self(self.0 - rhs)
    }
}

impl Sub<VirtAddr> for VirtAddr {
    type Output = u64;
    #[inline(always)]
    fn sub(self, rhs: VirtAddr) -> u64 {
        self.0 - rhs.0
    }
}

impl SubAssign<u64> for VirtAddr {
    #[inline(always)]
    fn sub_assign(&mut self, rhs: u64) {
        self.0 -= rhs;
    }
}

impl fmt::Debug for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VirtAddr({:#018x})", self.0)
    }
}

impl fmt::Display for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#018x}", self.0)
    }
}

impl fmt::LowerHex for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0, f)
    }
}
