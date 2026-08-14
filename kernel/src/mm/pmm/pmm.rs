use crate::mm::pmm::buddy::{BuddyAllocator, Page, PageFlags};
use crate::mm::types::PhysAddr;
use crate::sync::spinlock::Spinlock;

pub struct PhysicalMemoryManagement {
    allocator: Spinlock<Option<BuddyAllocator>>,
}

impl PhysicalMemoryManagement {
    pub const fn new() -> Self {
        Self {
            allocator: Spinlock::new(None),
        }
    }

    /// Initialize the physical memory manager using the Limine memory map.
    pub fn init(&self) {
        let memmap_response = crate::limine::MEMORY_MAP_REQUEST
            .get_response()
            .expect("PMM init: Limine memory map response is missing");
        let hhdm_response = crate::limine::HHDM_REQUEST
            .get_response()
            .expect("PMM init: Limine HHDM response is missing");

        let hhdm_offset = hhdm_response.offset();

        // Find the highest physical address to size our page map
        let mut max_paddr = 0;
        for entry in memmap_response.entries() {
            if entry.entry_type == limine::memory_map::EntryType::USABLE {
                let end = entry.base + entry.length;
                if end > max_paddr {
                    max_paddr = end;
                }
            }
        }

        if max_paddr == 0 {
            panic!("PMM init: no usable memory found in memory map");
        }

        let num_pages = (max_paddr / 4096) as usize;
        let page_map_size = num_pages * core::mem::size_of::<Page>();

        // Locate a usable region of memory above 16MB to hold the page_map array
        let mut page_map_phys = 0;
        for entry in memmap_response.entries() {
            if entry.entry_type == limine::memory_map::EntryType::USABLE {
                let region_start = entry.base.max(0x1000_000);
                let aligned_base = (region_start + 4095) & !4095;
                let aligned_end = (entry.base + entry.length) & !4095;
                if aligned_end > aligned_base
                    && (aligned_end - aligned_base) >= page_map_size as u64
                {
                    page_map_phys = aligned_base;
                    break;
                }
            }
        }

        if page_map_phys == 0 {
            panic!(
                "PMM init: failed to find a usable memory region large enough for page_map (needed {} bytes)",
                page_map_size
            );
        }

        // Initialize the page map array in memory using direct mapping (HHDM)
        let page_map_ptr = (page_map_phys + hhdm_offset) as *mut Page;
        for i in 0..num_pages {
            // SAFETY: page_map_ptr points to newly-allocated raw memory from a usable region, and we are initializing it.
            unsafe {
                page_map_ptr.add(i).write(Page::new());
            }
        }
        let page_map = unsafe { core::slice::from_raw_parts_mut(page_map_ptr, num_pages) };

        // Identify usable pages and count total pages, excluding the page_map's own pages
        let mut total_pages = 0;
        for entry in memmap_response.entries() {
            if entry.entry_type == limine::memory_map::EntryType::USABLE {
                let start = (entry.base + 4095) & !4095;
                let end = (entry.base + entry.length) & !4095;
                if start >= end {
                    continue;
                }

                let start_page = (start / 4096) as usize;
                let end_page = (end / 4096) as usize;

                for page_idx in start_page..end_page {
                    let paddr = (page_idx * 4096) as u64;
                    // Exclude real-mode / BIOS / firmware low memory below 16MB (0x1000000)
                    if paddr < 0x1000_000 {
                        continue;
                    }
                    // Exclude the pages containing the page map itself
                    if paddr >= page_map_phys && paddr < page_map_phys + page_map_size as u64 {
                        continue;
                    }

                    page_map[page_idx].flags.insert(PageFlags::USABLE);
                    total_pages += 1;
                }
            }
        }

        // Construct the BuddyAllocator
        let mut allocator = BuddyAllocator::new(page_map, hhdm_offset, total_pages);

        // Free all usable page ranges into the buddy allocator
        let mut start_page = None;
        for i in 0..num_pages {
            let is_usable = allocator.is_usable(i);
            match (is_usable, start_page) {
                (true, None) => {
                    start_page = Some(i);
                }
                (false, Some(start)) => {
                    allocator.free_initial_region(start, i);
                    start_page = None;
                }
                _ => {}
            }
        }
        if let Some(start) = start_page {
            allocator.free_initial_region(start, num_pages);
        }

        log::info!("Physical Memory Manager (PMM) Initialized.");
        log::info!(
            "  Total Usable Memory: {} MB ({} pages)",
            allocator.total_pages() * 4 / 1024,
            allocator.total_pages()
        );
        log::info!(
            "  Free Memory:         {} MB ({} pages)",
            allocator.free_pages() * 4 / 1024,
            allocator.free_pages()
        );

        allocator.debug_dump();

        *self.allocator.lock() = Some(allocator);
    }

    /// Allocate a block of physical memory of size 2^order pages.
    pub fn alloc_pages(&self, order: usize) -> Option<PhysAddr> {
        let mut guard = self.allocator.lock();
        if let Some(ref mut allocator) = *guard {
            allocator.alloc_pages(order)
        } else {
            None
        }
    }

    /// Free a block of physical memory of size 2^order pages.
    pub fn free_pages(&self, paddr: PhysAddr, order: usize) {
        assert!(
            paddr.is_aligned(4096),
            "pmm::free_pages: address is not page-aligned"
        );
        let mut guard = self.allocator.lock();
        if let Some(ref mut allocator) = *guard {
            let page_idx = allocator.get_page_index(paddr);
            if page_idx >= allocator.page_map_len() {
                log::error!("pmm::free_pages: page index {} out of bounds", page_idx);
                return;
            }

            if !allocator.is_usable(page_idx) {
                log::error!(
                    "pmm::free_pages: attempting to free non-usable page at physical address {:#x}",
                    paddr.as_u64()
                );
                return;
            }

            if allocator.is_free(page_idx) {
                log::error!(
                    "pmm::free_pages: double free detected at physical address {:#x}",
                    paddr.as_u64()
                );
                return;
            }

            let new_ref = allocator.dec_ref(paddr);
            if new_ref > 0 {
                // Page is still referenced by another shared mapping (COW)
                return;
            }

            // SAFETY: We checked that the page is usable, not already free, and ref_count reached 0.
            unsafe {
                allocator.free_block_internal(paddr, order);
            }
        }
    }

    /// Increment reference count for a physical page frame.
    pub fn inc_ref(&self, paddr: PhysAddr) {
        let mut guard = self.allocator.lock();
        if let Some(ref mut allocator) = *guard {
            allocator.inc_ref(paddr);
        }
    }

    /// Decrement reference count for a physical page frame without returning it to free list.
    pub fn dec_ref(&self, paddr: PhysAddr) -> u32 {
        let mut guard = self.allocator.lock();
        if let Some(ref mut allocator) = *guard {
            allocator.dec_ref(paddr)
        } else {
            0
        }
    }

    /// Query reference count for a physical page frame.
    pub fn get_ref(&self, paddr: PhysAddr) -> u32 {
        let guard = self.allocator.lock();
        if let Some(ref allocator) = *guard {
            allocator.get_ref(paddr)
        } else {
            0
        }
    }

    /// Helper to allocate a single page (order 0).
    pub fn alloc_page(&self) -> Option<PhysAddr> {
        self.alloc_pages(0)
    }

    /// Helper to free a single page (order 0).
    pub fn free_page(&self, paddr: PhysAddr) {
        self.free_pages(paddr, 0);
    }

    /// Get the total number of usable pages in the system.
    pub fn total_pages(&self) -> usize {
        let guard = self.allocator.lock();
        if let Some(ref allocator) = *guard {
            allocator.total_pages()
        } else {
            0
        }
    }

    /// Get the number of free pages in the system.
    pub fn free_pages_count(&self) -> usize {
        let guard = self.allocator.lock();
        if let Some(ref allocator) = *guard {
            allocator.free_pages()
        } else {
            0
        }
    }

    /// Debug dump the allocator status.
    pub fn debug_dump(&self) {
        let guard = self.allocator.lock();
        if let Some(ref allocator) = *guard {
            allocator.debug_dump();
        }
    }
}

pub static PMM: PhysicalMemoryManagement = PhysicalMemoryManagement::new();
