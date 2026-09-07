//! Kernel Virtual Heap Allocator (SLUB + Dynamic Fallback).
//!
//! Implements `core::alloc::GlobalAlloc` with predetermined slab classes for
//! allocations <= 2048 bytes, and direct buddy frame allocation fallback
//! for allocations > 2048 bytes (half a page).

use crate::mm::alloc::FRAME_ALLOCATOR;
use crate::mm::PhysAddr;
use crate::sync::Mutex;
use core::alloc::{GlobalAlloc, Layout};

/// Predetermined SLUB size classes (all powers of two up to 2048 bytes).
const SLAB_CLASSES: [usize; 7] = [32, 64, 128, 256, 512, 1024, 2048];

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
            // Request a new page from the buddy frame allocator
            let paddr = match FRAME_ALLOCATOR.lock().alloc_page() {
                Some(p) => p,
                None => return core::ptr::null_mut(),
            };

            let page_virt = (paddr.as_u64() + hhdm_offset) as usize;
            let objects_per_page = 4096 / self.size;

            // Link all objects in the newly allocated page
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
            // SAFETY: self.free_head is a non-null pointer to a FreeObject in a valid slab.
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
    classes: [SlabClass; 7],
}

unsafe impl Send for SlabAllocatorInner {}

impl SlabAllocatorInner {
    const fn new() -> Self {
        Self {
            classes: [
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

/// Global Slab Allocator combining SLUB caching with dynamic buddy fallback.
pub struct SlabAllocator {
    inner: Mutex<SlabAllocatorInner>,
}

impl SlabAllocator {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(SlabAllocatorInner::new()),
        }
    }

    /// Computes the smallest buddy order required to hold `pages` frames.
    #[inline(always)]
    fn pages_to_order(pages: usize) -> usize {
        let mut order = 0;
        while (1 << order) < pages {
            order += 1;
        }
        order
    }

    /// Finds the index of the slab class satisfying `req_size`.
    #[inline(always)]
    fn class_index(req_size: usize) -> Option<usize> {
        for (i, &size) in SLAB_CLASSES.iter().enumerate() {
            if size >= req_size {
                return Some(i);
            }
        }
        None
    }
}

unsafe impl GlobalAlloc for SlabAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }

        let hhdm = crate::mm::hhdm_offset();
        let target_size = core::cmp::max(layout.size(), layout.align());

        // Small allocations (<= 2048 bytes): route to SLUB classes
        if target_size <= 2048 {
            if let Some(idx) = Self::class_index(target_size) {
                let mut inner = self.inner.lock();
                return unsafe { inner.classes[idx].alloc(hhdm) };
            }
        }

        // Large allocations (> 2048 bytes): direct buddy frame allocation fallback
        let pages = (layout.size() + 4095) / 4096;
        let align_pages = (layout.align() + 4095) / 4096;
        let needed_pages = core::cmp::max(pages, align_pages);
        let order = Self::pages_to_order(needed_pages);

        match FRAME_ALLOCATOR.lock().alloc_pages(order) {
            Some(paddr) => (paddr.as_u64() + hhdm) as *mut u8,
            None => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() || layout.size() == 0 {
            return;
        }

        let hhdm = crate::mm::hhdm_offset();
        let target_size = core::cmp::max(layout.size(), layout.align());

        // Small allocations: return to SLUB cache
        if target_size <= 2048 {
            if let Some(idx) = Self::class_index(target_size) {
                let mut inner = self.inner.lock();
                unsafe { inner.classes[idx].free(ptr) };
                return;
            }
        }

        // Large allocations: return directly to buddy frame pool
        let pages = (layout.size() + 4095) / 4096;
        let align_pages = (layout.align() + 4095) / 4096;
        let needed_pages = core::cmp::max(pages, align_pages);
        let order = Self::pages_to_order(needed_pages);

        let paddr = PhysAddr::new((ptr as u64) - hhdm);
        FRAME_ALLOCATOR.lock().free_pages(paddr, order);
    }
}

#[global_allocator]
pub static ALLOCATOR: SlabAllocator = SlabAllocator::new();
