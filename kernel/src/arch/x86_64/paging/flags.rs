use crate::mm::paging::MapFlags;

pub const PAGE_PRESENT: u64 = 1 << 0;
pub const PAGE_WRITABLE: u64 = 1 << 1;
pub const PAGE_USER: u64 = 1 << 2;
pub const PAGE_PWT: u64 = 1 << 3; // Write-Through
pub const PAGE_PCD: u64 = 1 << 4; // Cache-Disable
pub const PAGE_ACCESSED: u64 = 1 << 5;
pub const PAGE_DIRTY: u64 = 1 << 6;
pub const PAGE_HUGE: u64 = 1 << 7;
pub const PAGE_GLOBAL: u64 = 1 << 8;
pub const PAGE_NO_EXECUTE: u64 = 1 << 63;

/// Convert generic architecture-independent `MapFlags` to x86_64 page table entry raw flags.
pub fn translate_flags(flags: MapFlags) -> u64 {
    let mut entry_flags = PAGE_PRESENT; // Present (bit 0) is set for valid mappings
    if flags.contains(MapFlags::WRITE) {
        entry_flags |= PAGE_WRITABLE;
    }
    if flags.contains(MapFlags::USER) {
        entry_flags |= PAGE_USER;
    }
    if flags.contains(MapFlags::NO_CACHE) {
        entry_flags |= PAGE_PCD;
    }
    if !flags.contains(MapFlags::EXECUTE) {
        entry_flags |= PAGE_NO_EXECUTE;
    }
    entry_flags
}
