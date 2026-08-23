//! Page Table Flags and Page Fault Error Codes for PetraOS.
//!
//! Provides architecture-agnostic representations for hardware paging permissions and fault reasons.

use bitflags::bitflags;

bitflags! {
    /// Flags specifying permissions, caching, and state for page table entries.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct PageTableFlags: u64 {
        /// Page is present in memory.
        const PRESENT = 1 << 0;
        /// Page is writable (read/write).
        const WRITABLE = 1 << 1;
        /// Page is accessible from user mode (Ring 3).
        const USER_ACCESSIBLE = 1 << 2;
        /// Page level write-through caching enabled.
        const WRITE_THROUGH = 1 << 3;
        /// Page level cache disabled (useful for MMIO).
        const NO_CACHE = 1 << 4;
        /// Page has been accessed by CPU read or write.
        const ACCESSED = 1 << 5;
        /// Page has been written to by CPU.
        const DIRTY = 1 << 6;
        /// Huge page indicator (2 MiB in PD or 1 GiB in PDPT).
        const HUGE_PAGE = 1 << 7;
        /// Global page (not flushed from TLB on CR3 reload).
        const GLOBAL = 1 << 8;
        /// Custom OS flag 1 (used for Copy-On-Write).
        const BIT_9 = 1 << 9;
        /// Custom OS flag 2.
        const BIT_10 = 1 << 10;
        /// Custom OS flag 3.
        const BIT_11 = 1 << 11;
        /// Execute-Disable bit (prevents instruction fetching).
        const NO_EXECUTE = 1 << 63;
    }
}

/// Constant alias for Copy-On-Write flag on page table entries.
pub const COW_FLAG: PageTableFlags = PageTableFlags::BIT_9;

bitflags! {
    /// Error code pushed by CPU during a Page Fault exception (#PF).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct PageFaultErrorCode: u64 {
        /// Set if fault was caused by a protection violation (page was present),
        /// cleared if fault was caused by a not-present page.
        const PROTECTION_VIOLATION = 1 << 0;
        /// Set if access causing fault was a write, cleared if read.
        const CAUSED_BY_WRITE = 1 << 1;
        /// Set if fault occurred in user mode (Ring 3), cleared if supervisor mode (Ring 0).
        const USER_MODE = 1 << 2;
        /// Set if reserved bits in page table entry were set.
        const MALFORMED_TABLE = 1 << 3;
        /// Set if fault was caused by an instruction fetch.
        const INSTRUCTION_FETCH = 1 << 4;
        /// Set if protection key violation.
        const PROTECTION_KEY = 1 << 5;
        /// Set if shadow stack access fault.
        const SHADOW_STACK = 1 << 6;
        /// Set if SGX access fault.
        const SGX = 1 << 15;
    }
}
