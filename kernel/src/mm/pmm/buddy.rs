use crate::mm::alloc::freelist::{IntrusiveList, IntrusiveNode};
use x86_64::PhysAddr;

pub const MAX_ORDER: usize = 16;

#[derive(Debug, Clone, Copy)]
pub struct Page {
    pub flags: PageFlags,
    pub order: u8,
    pub ref_count: u32,
}

impl Page {
    pub const fn new() -> Self {
        Self {
            flags: PageFlags::empty(),
            order: 0,
            ref_count: 0,
        }
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PageFlags: u8 {
        const USABLE = 1 << 0;
        const FREE   = 1 << 1;
    }
}

pub struct BuddyAllocator {
    free_lists: [IntrusiveList; MAX_ORDER],
    page_map: &'static mut [Page],
    hhdm_offset: u64,
    total_pages: usize,
    free_pages: usize,
}

unsafe impl Send for BuddyAllocator {}
unsafe impl Sync for BuddyAllocator {}

impl BuddyAllocator {
    pub fn new(page_map: &'static mut [Page], hhdm_offset: u64, total_pages: usize) -> Self {
        Self {
            free_lists: [IntrusiveList::new(); MAX_ORDER],
            page_map,
            hhdm_offset,
            total_pages,
            free_pages: 0,
        }
    }

    pub fn get_page_index(&self, paddr: PhysAddr) -> usize {
        (paddr.as_u64() / 4096) as usize
    }

    pub fn get_page_paddr(&self, index: usize) -> PhysAddr {
        PhysAddr::new((index * 4096) as u64)
    }

    pub fn is_usable(&self, index: usize) -> bool {
        self.page_map[index].flags.contains(PageFlags::USABLE)
    }

    pub fn is_free(&self, index: usize) -> bool {
        self.page_map[index].flags.contains(PageFlags::FREE)
    }

    pub fn page_map_len(&self) -> usize {
        self.page_map.len()
    }

    pub fn total_pages(&self) -> usize {
        self.total_pages
    }

    pub fn free_pages(&self) -> usize {
        self.free_pages
    }

    pub fn inc_ref(&mut self, paddr: PhysAddr) {
        let idx = self.get_page_index(paddr);
        if idx < self.page_map.len() {
            // Use saturating_add to prevent u32 overflow causing a premature free.
            self.page_map[idx].ref_count = self.page_map[idx].ref_count.saturating_add(1);
        }
    }

    pub fn dec_ref(&mut self, paddr: PhysAddr) -> u32 {
        let idx = self.get_page_index(paddr);
        if idx < self.page_map.len() {
            if self.page_map[idx].ref_count > 0 {
                self.page_map[idx].ref_count -= 1;
            }
            self.page_map[idx].ref_count
        } else {
            0
        }
    }

    pub fn get_ref(&self, paddr: PhysAddr) -> u32 {
        let idx = self.get_page_index(paddr);
        if idx < self.page_map.len() {
            self.page_map[idx].ref_count
        } else {
            0
        }
    }

    /// Safely split a larger free block and remove the node from the free list.
    unsafe fn remove_block(&mut self, block: *mut IntrusiveNode, order: usize) {
        unsafe {
            self.free_lists[order].remove(block);
        }
    }

    /// Prepend a free block to the free list of the given order.
    unsafe fn insert_block(&mut self, paddr: PhysAddr, order: usize) {
        let block_node = (paddr.as_u64() + self.hhdm_offset) as *mut IntrusiveNode;
        unsafe {
            self.free_lists[order].push_front(block_node);
        }
    }

    /// Allocate $2^{\text{order}}$ physical pages.
    pub fn alloc_pages(&mut self, order: usize) -> Option<PhysAddr> {
        if order >= MAX_ORDER {
            return None;
        }

        // Find the smallest order >= requested order that contains free blocks
        let mut target_order = order;
        while target_order < MAX_ORDER {
            if !self.free_lists[target_order].is_empty() {
                break;
            }
            target_order += 1;
        }

        if target_order == MAX_ORDER {
            return None; // Out of physical memory
        }

        // Dequeue the free block from the target order's free list
        let block_node = self.free_lists[target_order].head;
        unsafe {
            self.remove_block(block_node, target_order);
        }

        let paddr = PhysAddr::new(block_node as u64 - self.hhdm_offset);
        let page_idx = self.get_page_index(paddr);

        // Clear the metadata: the block is no longer free
        self.page_map[page_idx].flags.remove(PageFlags::FREE);
        self.page_map[page_idx].order = 0;

        // Split the block repeatedly down to the requested order
        let mut cur_order = target_order;
        while cur_order > order {
            cur_order -= 1;

            // Compute the starting address of the buddy block (second half)
            let buddy_paddr = paddr + ((1 << cur_order) * 4096u64);
            let buddy_idx = self.get_page_index(buddy_paddr);

            if buddy_idx < self.page_map.len() {
                // Mark the buddy block as free with order `cur_order`
                self.page_map[buddy_idx].order = cur_order as u8;
                self.page_map[buddy_idx].flags.insert(PageFlags::FREE);

                // Add the buddy block to the free list for `cur_order`
                unsafe {
                    self.insert_block(buddy_paddr, cur_order);
                }
            }
        }

        // Set the order of the allocated block on its head page
        self.page_map[page_idx].order = order as u8;
        // Only the head page carries the refcount; the buddy allocator's free path
        // always uses the head page index via get_page_index(paddr).
        if page_idx < self.page_map.len() {
            self.page_map[page_idx].ref_count = 1;
        }

        self.free_pages = self.free_pages.saturating_sub(1 << order);

        Some(paddr)
    }

    /// Free a block of physical pages, merging it with its buddy block if possible.
    ///
    /// # Safety
    /// Caller must guarantee that `paddr` is valid, usable, and currently allocated.
    pub unsafe fn free_block_internal(&mut self, paddr: PhysAddr, mut order: usize) {
        let mut page_idx = self.get_page_index(paddr);

        // Guard: never insert a non-usable page into the free list.
        if page_idx >= self.page_map.len() || !self.page_map[page_idx].flags.contains(PageFlags::USABLE) {
            log::error!("free_block_internal: attempted to free non-usable page at {:#x}", paddr.as_u64());
            return;
        }

        let initial_order = order;

        while order < MAX_ORDER - 1 {
            let buddy_idx = page_idx ^ (1 << order);
            let block_pages = 1 << order;
            if buddy_idx >= self.page_map.len() || buddy_idx + block_pages > self.page_map.len() {
                break;
            }

            let buddy_page = &self.page_map[buddy_idx];
            if buddy_page.flags.contains(PageFlags::USABLE)
                && buddy_page.flags.contains(PageFlags::FREE)
                && buddy_page.order as usize == order
            {
                // Remove the buddy from its current free list
                let buddy_paddr = self.get_page_paddr(buddy_idx);
                let buddy_node = (buddy_paddr.as_u64() + self.hhdm_offset) as *mut IntrusiveNode;
                unsafe {
                    self.remove_block(buddy_node, order);
                }

                // Clear the buddy's free status and order
                self.page_map[buddy_idx].flags.remove(PageFlags::FREE);
                self.page_map[buddy_idx].order = 0;

                // Adjust page index to the merged start boundary, increment order
                page_idx = page_idx & !(1 << order);
                order += 1;
            } else {
                break;
            }
        }

        // Set new order and mark the merged block as free
        self.page_map[page_idx].order = order as u8;
        self.page_map[page_idx].flags.insert(PageFlags::FREE);

        // Add the merged block to the free list of the new order
        let merged_paddr = self.get_page_paddr(page_idx);
        unsafe {
            self.insert_block(merged_paddr, order);
        }

        self.free_pages += 1 << initial_order;
    }

    /// Partition a contiguous range of page frames into buddy blocks and free them.
    pub fn free_initial_region(&mut self, start_page: usize, end_page: usize) {
        let mut cur = start_page;
        while cur < end_page {
            let mut order = 0;
            while order < MAX_ORDER - 1 {
                let block_size = 1 << (order + 1);
                if cur % block_size != 0 || cur + block_size > end_page {
                    break;
                }
                order += 1;
            }

            let paddr = PhysAddr::new((cur * 4096) as u64);
            unsafe {
                self.free_block_internal(paddr, order);
            }
            cur += 1 << order;
        }
    }

    /// Log the current list of free blocks at each order for diagnostic purposes.
    pub fn debug_dump(&self) {
        log::info!("PMM Buddy Free List Status:");
        for order in 0..MAX_ORDER {
            let mut count = 0;
            let mut curr = self.free_lists[order].head;
            while !curr.is_null() {
                count += 1;
                unsafe {
                    curr = (*curr).next;
                }
            }
            if count > 0 {
                let size_kb = (1 << order) * 4;
                if size_kb >= 1024 {
                    log::info!(
                        "  Order {:2}: {:4} blocks ({} MB)",
                        order,
                        count,
                        size_kb / 1024
                    );
                } else {
                    log::info!("  Order {:2}: {:4} blocks ({} KB)", order, count, size_kb);
                }
            }
        }
    }
}
