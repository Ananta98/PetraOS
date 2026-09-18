//! Early Bootstrapping Bump Allocator.
//!
//! Provides a minimal linear bump allocator active during early kernel boot
//! before full buddy and virtual memory subsystems are initialized.
//! Used primarily to carve out the `PageFrameMetadata` tracking array.

use super::buddy::{PageFrameMetadata, PAGE_SIZE};
use core::mem::size_of;

/// Bootstrap allocation result containing the allocated metadata slice
/// and the physical address range consumed.
pub struct EarlyBumpResult {
    pub metadata: &'static mut [PageFrameMetadata],
    pub metadata_phys_start: u64,
    pub metadata_phys_end: u64,
    pub total_pages: usize,
    pub max_paddr: u64,
}

/// Carves out the physical page frame metadata array from early usable RAM.
///
/// # Panics
/// Panics if the Limine memory map is missing or no usable RAM region large enough
/// is available above 1 MiB.
pub fn early_allocate_metadata(hhdm_offset: u64) -> EarlyBumpResult {
    let memmap_response = crate::limine::MEMORY_MAP_REQUEST
        .get_response()
        .expect("Early bump allocator: Limine memory map response is missing");

    // 1. Find the highest physical address to size our page metadata array
    let mut max_paddr: u64 = 0;
    for entry in memmap_response.entries() {
        if entry.entry_type == limine::memory_map::EntryType::USABLE {
            let end = entry.base + entry.length;
            if end > max_paddr {
                max_paddr = end;
            }
        }
    }

    if max_paddr == 0 {
        panic!("Early bump allocator: no usable memory found in memory map");
    }

    let total_pages = ((max_paddr + PAGE_SIZE - 1) / PAGE_SIZE) as usize;
    let metadata_bytes = total_pages * size_of::<PageFrameMetadata>();
    let metadata_aligned_bytes = (metadata_bytes + (PAGE_SIZE as usize) - 1) & !(PAGE_SIZE as usize - 1);

    // 2. Locate a usable region above 1 MiB large enough for metadata
    let mut chosen_phys_base: u64 = 0;
    for entry in memmap_response.entries() {
        if entry.entry_type == limine::memory_map::EntryType::USABLE {
            let region_start = entry.base.max(0x100_000); // Exclude sub-1MB low memory
            let aligned_start = (region_start + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
            let aligned_end = (entry.base + entry.length) & !(PAGE_SIZE - 1);

            if aligned_end > aligned_start
                && (aligned_end - aligned_start) >= metadata_aligned_bytes as u64
            {
                chosen_phys_base = aligned_start;
                break;
            }
        }
    }

    if chosen_phys_base == 0 {
        panic!(
            "Early bump allocator: failed to locate usable region for metadata (needed {} bytes)",
            metadata_aligned_bytes
        );
    }

    let metadata_phys_end = chosen_phys_base + metadata_aligned_bytes as u64;
    let metadata_virt_ptr = (chosen_phys_base + hhdm_offset) as *mut PageFrameMetadata;

    // 3. Initialize all metadata entries to default (zeroed)
    // SAFETY: We carved out an exclusive, usable physical memory region and map it via HHDM.
    unsafe {
        core::ptr::write_bytes(metadata_virt_ptr as *mut u8, 0, metadata_aligned_bytes);
        let slice = core::slice::from_raw_parts_mut(metadata_virt_ptr, total_pages);
        for item in slice.iter_mut() {
            *item = PageFrameMetadata::new();
        }

        EarlyBumpResult {
            metadata: slice,
            metadata_phys_start: chosen_phys_base,
            metadata_phys_end,
            total_pages,
            max_paddr,
        }
    }
}
