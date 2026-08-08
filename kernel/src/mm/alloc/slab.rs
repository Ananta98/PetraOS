use crate::mm::types::PhysAddr;
use crate::mm::alloc::freelist::{IntrusiveList, IntrusiveNode};
use crate::mm::PMM;
use crate::sync::spinlock::Spinlock;
use core::alloc::{GlobalAlloc, Layout};

struct FreeBlock {
    next: *mut FreeBlock,
}

#[allow(dead_code)]
#[repr(C)]
struct Slab {
    node: IntrusiveNode,
    free_list: *mut FreeBlock,
    allocated_count: usize,
    total_count: usize,
}

impl Slab {
    /// Initialize a new slab header and its free blocks inside the page.
    ///
    /// # Safety
    /// `page_virt` must be a valid virtual address to a newly-allocated 4 KB page.
    unsafe fn init(page_virt: usize, block_size: usize, alignment: usize) -> *mut Self {
        let slab_ptr = page_virt as *mut Self;

        let slab_size = core::mem::size_of::<Self>();
        let first_block_addr = (page_virt + slab_size + alignment - 1) & !(alignment - 1);
        let total_count = (page_virt + 4096 - first_block_addr) / block_size;

        let mut free_list: *mut FreeBlock = core::ptr::null_mut();
        unsafe {
            for i in (0..total_count).rev() {
                let block_ptr = (first_block_addr + i * block_size) as *mut FreeBlock;
                (*block_ptr).next = free_list;
                free_list = block_ptr;
            }

            slab_ptr.write(Self {
                node: IntrusiveNode::new(),
                free_list,
                allocated_count: 0,
                total_count,
            });
        }

        slab_ptr
    }
}

pub struct KmemCache {
    name: &'static str,
    object_size: usize,
    alignment: usize,
    slabs_full: IntrusiveList,
    slabs_partial: IntrusiveList,
    slabs_empty: IntrusiveList,
}

impl KmemCache {
    pub const fn new(name: &'static str, object_size: usize, alignment: usize) -> Self {
        Self {
            name,
            object_size,
            alignment,
            slabs_full: IntrusiveList::new(),
            slabs_partial: IntrusiveList::new(),
            slabs_empty: IntrusiveList::new(),
        }
    }

    /// Allocate an object from the cache.
    ///
    /// # Safety
    /// Caller must ensure that self is locked or accessed exclusively.
    pub unsafe fn alloc(&mut self, hhdm_offset: u64) -> *mut u8 {
        // Find a slab that has free blocks:
        // First, check partial slabs. If none, check empty slabs.
        let mut slab_node = self.slabs_partial.head;
        if slab_node.is_null() {
            slab_node = self.slabs_empty.head;
        }

        // If no usable slab exists, allocate a new page from PMM
        if slab_node.is_null() {
            let page_phys = match PMM.alloc_page() {
                Some(paddr) => paddr,
                None => return core::ptr::null_mut(),
            };

            let page_virt = (page_phys.as_u64() + hhdm_offset) as usize;
            let new_slab = unsafe { Slab::init(page_virt, self.object_size, self.alignment) };

            unsafe {
                self.slabs_empty.push_front(&mut (*new_slab).node);
            }
            slab_node = unsafe { &mut (*new_slab).node };
        }

        let slab = slab_node as *mut Slab;

        unsafe {
            let block = (*slab).free_list;
            (*slab).free_list = (*block).next;

            // Remove slab from its current list
            if (*slab).allocated_count == 0 {
                self.slabs_empty.remove(slab_node);
            } else {
                self.slabs_partial.remove(slab_node);
            }

            (*slab).allocated_count += 1;

            // Transition to new list
            if (*slab).allocated_count == (*slab).total_count {
                self.slabs_full.push_front(slab_node);
            } else {
                self.slabs_partial.push_front(slab_node);
            }

            block as *mut u8
        }
    }

    /// Free an object back to this cache.
    ///
    /// # Safety
    /// Caller must ensure that self is locked or accessed exclusively, and that `ptr` belongs to this cache.
    pub unsafe fn free(&mut self, ptr: *mut u8, hhdm_offset: u64) {
        let page_start = (ptr as usize) & !4095;
        let slab = page_start as *mut Slab;
        unsafe {
            let slab_node = &mut (*slab).node as *mut IntrusiveNode;
            let block = ptr as *mut FreeBlock;
            (*block).next = (*slab).free_list;
            (*slab).free_list = block;

            // Remove slab from its current list (full or partial)
            if (*slab).allocated_count == (*slab).total_count {
                self.slabs_full.remove(slab_node);
            } else {
                self.slabs_partial.remove(slab_node);
            }

            (*slab).allocated_count -= 1;

            // Transition slab to empty or partial list
            if (*slab).allocated_count == 0 {
                // Free the page back to PMM
                let paddr = PhysAddr(page_start as u64 - hhdm_offset);
                PMM.free_page(paddr);
            } else {
                self.slabs_partial.push_front(slab_node);
            }
        }
    }
}

struct SlabAllocatorInner {
    caches: [KmemCache; 9],
}

unsafe impl Send for SlabAllocatorInner {}
unsafe impl Sync for SlabAllocatorInner {}

impl SlabAllocatorInner {
    const fn new() -> Self {
        Self {
            caches: [
                KmemCache::new("kmalloc-8", 8, 8),
                KmemCache::new("kmalloc-16", 16, 16),
                KmemCache::new("kmalloc-32", 32, 32),
                KmemCache::new("kmalloc-64", 64, 64),
                KmemCache::new("kmalloc-128", 128, 128),
                KmemCache::new("kmalloc-256", 256, 256),
                KmemCache::new("kmalloc-512", 512, 512),
                KmemCache::new("kmalloc-1024", 1024, 1024),
                KmemCache::new("kmalloc-2048", 2048, 2048),
            ],
        }
    }
}

pub struct SlabAllocator {
    inner: Spinlock<SlabAllocatorInner>,
}

impl SlabAllocator {
    pub const fn new() -> Self {
        Self {
            inner: Spinlock::new(SlabAllocatorInner::new()),
        }
    }

    pub fn size_to_order(pages: usize) -> usize {
        let mut order = 0;
        while (1 << order) < pages {
            order += 1;
        }
        order
    }
}

unsafe impl GlobalAlloc for SlabAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        let hhdm_offset = crate::mm::hhdm_offset();

        if size > 2048 {
            // Allocate pages directly from PMM
            let align_pages = (align + 4095) / 4096;
            let pages_needed = core::cmp::max((size + 4095) / 4096, align_pages);
            let order = Self::size_to_order(pages_needed);

            return match PMM.alloc_pages(order) {
                Some(paddr) => (paddr.as_u64() + hhdm_offset) as *mut u8,
                None => core::ptr::null_mut(),
            };
        }

        let mut inner = self.inner.lock();

        let cache = match inner
            .caches
            .iter_mut()
            .find(|c| c.object_size >= size && c.alignment >= align)
        {
            Some(c) => c,
            None => return core::ptr::null_mut(),
        };

        unsafe { cache.alloc(hhdm_offset) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }

        let size = layout.size();
        let align = layout.align();

        let hhdm_offset = crate::mm::hhdm_offset();

        if size > 2048 {
            // Large allocation, free pages back to PMM
            let align_pages = (align + 4095) / 4096;
            let pages_needed = core::cmp::max((size + 4095) / 4096, align_pages);
            let order = Self::size_to_order(pages_needed);

            let paddr = PhysAddr(ptr as u64 - hhdm_offset);
            PMM.free_pages(paddr, order);
            return;
        }

        let mut inner = self.inner.lock();

        let cache = match inner
            .caches
            .iter_mut()
            .find(|c| c.object_size >= size && c.alignment >= align)
        {
            Some(c) => c,
            None => return,
        };

        unsafe { cache.free(ptr, hhdm_offset) }
    }
}

#[global_allocator]
static ALLOCATOR: SlabAllocator = SlabAllocator::new();
