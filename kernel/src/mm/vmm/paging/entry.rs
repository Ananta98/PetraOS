//! Raw Page Table Entry Abstraction for PetraOS.
//!
//! Provides `PageTableEntry` for manipulating 64-bit physical addresses and flags within page tables.

use crate::mm::vmm::address::PhysAddr;
use crate::mm::vmm::flags::PageTableFlags;

/// A 64-bit hardware page table entry.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct PageTableEntry(pub u64);

impl PageTableEntry {
    /// Mask for the physical address stored in the entry (bits 12..51).
    pub const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

    /// Mask for the permission and status flags (bits 0..11 and bit 63).
    pub const FLAGS_MASK: u64 = 0x8000_0000_0000_0FFF;

    /// Create an empty (not present) page table entry.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Creates a new page table entry with the given physical address and flags.
    #[inline(always)]
    pub const fn new(addr: PhysAddr, flags: PageTableFlags) -> Self {
        Self((addr.as_u64() & Self::ADDR_MASK) | flags.bits())
    }

    /// Returns the raw 64-bit representation of the entry.
    #[inline(always)]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Returns the physical address pointing to the frame or next page table.
    #[inline(always)]
    pub fn addr(self) -> PhysAddr {
        PhysAddr::new(self.0 & Self::ADDR_MASK)
    }

    /// Returns the flags configured for this entry.
    #[inline(always)]
    pub fn flags(self) -> PageTableFlags {
        PageTableFlags::from_bits_truncate(self.0)
    }

    /// Sets the physical address and flags for this entry.
    #[inline(always)]
    pub fn set(&mut self, addr: PhysAddr, flags: PageTableFlags) {
        self.0 = (addr.as_u64() & Self::ADDR_MASK) | flags.bits();
    }

    /// Updates only the flags of the entry, preserving the physical address.
    #[inline(always)]
    pub fn set_flags(&mut self, flags: PageTableFlags) {
        self.0 = (self.0 & Self::ADDR_MASK) | flags.bits();
    }

    /// Updates only the address of the entry, preserving the flags.
    #[inline(always)]
    pub fn set_addr(&mut self, addr: PhysAddr) {
        self.0 = (addr.as_u64() & Self::ADDR_MASK) | (self.0 & Self::FLAGS_MASK);
    }

    /// Returns true if the PRESENT flag is set.
    #[inline(always)]
    pub fn is_present(self) -> bool {
        self.flags().contains(PageTableFlags::PRESENT)
    }

    /// Returns true if the HUGE_PAGE flag is set.
    #[inline(always)]
    pub fn is_huge(self) -> bool {
        self.flags().contains(PageTableFlags::HUGE_PAGE)
    }

    /// Clears the entry completely (resets to 0).
    #[inline(always)]
    pub fn clear(&mut self) {
        self.0 = 0;
    }
}

impl core::fmt::Debug for PageTableEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "PageTableEntry(addr: {:?}, flags: {:?})",
            self.addr(),
            self.flags()
        )
    }
}
