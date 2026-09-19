//! Kernel SLUB Allocator with direct Buddy Frame Fallback.
//!
//! Implements `core::alloc::GlobalAlloc` with predetermined slab classes for
//! allocations <= 2048 bytes, and direct contiguous frame allocation from
//! [`FRAME_ALLOCATOR`] for larger allocations.
//!
//! Replaces external heap structures with a direct, fragmentation-resistant
//! design that eliminates circular allocator dependencies and cross-chunk
//! buddy XOR corruption.
//!
//!
use super::FRAME_ALLOCATOR;
use super::buddy::{FRAME_BUDDY_ORDERS, PAGE_SIZE};
use crate::mm::PhysAddr;
use crate::sync::Mutex;
use core::alloc::{GlobalAlloc, Layout};

/// Predetermined SLUB size classes (all powers of two from 16 to 2048 bytes).
pub const SLAB_CLASSES: [usize; 8] = [16, 32, 64, 128, 256, 512, 1024, 2048];

/// Intrusive free object node stored directly inside unallocated slab slots.
#[repr(C)]
struct FreeObject {
    next: *mut FreeObject,
}

/// A size class cache holding a singly-linked list of free objects.
struct SlabClass {
    size: usize,
    free_head: *mut FreeObject,
}

unsafe impl Send for SlabClass {}

impl SlabClass {
    const fn new(size: usize) -> Self {
        Self {
            size,
            free_head: core::ptr::null_mut(),
        }
    }

    /// Allocate an object from this size class, requesting a new 4 KiB frame if empty.
    unsafe fn alloc(&mut self, hhdm_offset: u64) -> *mut u8 {
        if self.free_head.is_null() {
            let paddr = match FRAME_ALLOCATOR.lock().alloc_page() {
                Some(p) => p,
                None => return core::ptr::null_mut(),
            };

            let page_virt = (paddr.as_u64() + hhdm_offset) as usize;
            let objects_per_page = (PAGE_SIZE as usize) / self.size;

            // Link all objects in the newly allocated page into free list
            let mut head: *mut FreeObject = core::ptr::null_mut();
            for i in (0..objects_per_page).rev() {
                let obj_ptr = (page_virt + i * self.size) as *mut FreeObject;
                // SAFETY: obj_ptr is within the newly allocated, non-overlapping page frame mapped via HHDM.
                unsafe {
                    (*obj_ptr).next = head;
                }
                head = obj_ptr;
            }
            self.free_head = head;
        }

        let obj = self.free_head;
        if !obj.is_null() {
            // SAFETY: self.free_head is a non-null pointer to a valid FreeObject in a slab frame.
            unsafe {
                self.free_head = (*obj).next;
            }
        }
        obj as *mut u8
    }

    /// Free an object back to this size class.
    unsafe fn free(&mut self, ptr: *mut u8) {
        let obj = ptr as *mut FreeObject;
        // SAFETY: Caller guarantees ptr was allocated by this slab cache and is exclusive.
        unsafe {
            (*obj).next = self.free_head;
        }
        self.free_head = obj;
    }
}

/// Inner state of the SLUB allocator managing all predetermined size classes.
struct SlabAllocatorInner {
    classes: [SlabClass; 8],
}

unsafe impl Send for SlabAllocatorInner {}

impl SlabAllocatorInner {
    const fn new() -> Self {
        Self {
            classes: [
                SlabClass::new(16),
                SlabClass::new(32),
                SlabClass::new(64),
                SlabClass::new(128),
                SlabClass::new(256),
                SlabClass::new(512),
                SlabClass::new(1024),
                SlabClass::new(2048),
            ],
        }
    }
}

/// Global kernel allocator: SLUB caches for <= 2048 bytes, direct buddy frames for > 2048 bytes.
pub struct SlabAllocator {
    inner: Mutex<SlabAllocatorInner>,
}

impl SlabAllocator {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(SlabAllocatorInner::new()),
        }
    }

    /// Finds the index of the smallest slab class satisfying `req_size`.
    #[inline(always)]
    fn class_index(req_size: usize) -> Option<usize> {
        for (i, &size) in SLAB_CLASSES.iter().enumerate() {
            if size >= req_size {
                return Some(i);
            }
        }
        None
    }

    /// Smallest buddy order holding `pages` frames.
    #[inline(always)]
    fn pages_to_order(pages: usize) -> usize {
        let mut order = 0;
        let mut run = 1usize;
        while run < pages {
            run <<= 1;
            order += 1;
        }
        order
    }
}

unsafe impl GlobalAlloc for SlabAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }

        let hhdm = crate::mm::hhdm_offset();
        let target_size = core::cmp::max(layout.size(), layout.align());

        // Small allocations (<= 2048 bytes): route to SLUB size classes
        if target_size <= 2048 {
            if let Some(idx) = Self::class_index(target_size) {
                let mut inner = self.inner.lock();
                // SAFETY: inner lock held, hhdm valid direct map offset.
                return unsafe { inner.classes[idx].alloc(hhdm) };
            }
        }

        // Large allocations (> 2048 bytes): allocate contiguous frames directly from buddy frame allocator
        let pages = (target_size + (PAGE_SIZE as usize) - 1) / (PAGE_SIZE as usize);
        let order = Self::pages_to_order(pages);
        if order >= FRAME_BUDDY_ORDERS {
            log::error!(
                "SlabAllocator::alloc: requested size {} exceeds max order {}",
                target_size,
                FRAME_BUDDY_ORDERS - 1
            );
            return core::ptr::null_mut();
        }

        let paddr = match FRAME_ALLOCATOR.lock().alloc_pages(order) {
            Some(p) => p,
            None => return core::ptr::null_mut(),
        };

        (paddr.as_u64() + hhdm) as *mut u8
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() || layout.size() == 0 {
            return;
        }

        let target_size = core::cmp::max(layout.size(), layout.align());

        // Small allocations (<= 2048 bytes): return to SLUB cache
        if target_size <= 2048 {
            if let Some(idx) = Self::class_index(target_size) {
                let mut inner = self.inner.lock();
                // SAFETY: ptr was allocated from this size class and inner is locked.
                unsafe { inner.classes[idx].free(ptr) };
                return;
            }
        }

        // Large allocations (> 2048 bytes): return contiguous frames directly to buddy frame allocator
        let hhdm = crate::mm::hhdm_offset();
        let paddr = PhysAddr::new((ptr as u64) - hhdm);
        let pages = (target_size + (PAGE_SIZE as usize) - 1) / (PAGE_SIZE as usize);
        let order = Self::pages_to_order(pages);
        FRAME_ALLOCATOR.lock().free_pages(paddr, order);
    }
}

#[global_allocator]
pub static ALLOCATOR: SlabAllocator = SlabAllocator::new();

/// Initializes the slab allocator subsystem.
pub fn init() {
    log::info!("SlabAllocator: initialized (classes 16..2048, large fallback to buddy)");
}
