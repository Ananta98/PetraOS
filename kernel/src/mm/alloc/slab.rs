//! Kernel SLUB Allocator with buddy-Heap fallback.
//!
//! Implements `core::alloc::GlobalAlloc` with predetermined slab classes for
//! allocations <= 2048 bytes, and a `buddy_system_allocator::Heap` fallback
//! (grown on demand from the frame allocator) for larger allocations.
//!
//! Re-entrancy protocol (shared with `super::buddy`): the frame allocator
//! keeps its free index in `BTreeSet`s whose nodes come from this global
//! allocator, which itself grows from frames. To break the cycle:
//! - a static, frame-independent early heap backs small allocations made
//!   while [`IN_FRAME_ALLOC`] is set (i.e. from inside frame critical
//!   sections);
//! - [`SlabAllocator::dealloc`] routes frees by address (early-heap range
//!   check first), so every object returns to the allocator that issued it
//!   under any interleaving — staleness of the flag is always sound.
//!
//! Lock ordering is slab → frames, heap → frames, or early-heap alone; the
//! slab and fallback-heap locks are never held together.

use super::FRAME_ALLOCATOR;
use super::buddy::{FRAME_BUDDY_ORDERS, PAGE_SIZE};
use crate::sync::Mutex;
use buddy_system_allocator::Heap;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Predetermined SLUB size classes (all powers of two up to 2048 bytes).
const SLAB_CLASSES: [usize; 7] = [32, 64, 128, 256, 512, 1024, 2048];

/// Maximum fallback-heap order: largest heap block is `2^(ORDER - 1)` bytes
/// (`32` → 2 GiB, covering any sane single large allocation).
pub const HEAP_MAX_ORDER: usize = 32;

/// Minimum bytes pulled from the frame allocator per fallback-heap growth step.
const RESCUE_MIN_BYTES: usize = 64 * 1024;

/// Static early-heap size. Serves only small index-set nodes issued from
/// inside frame critical sections; 1 MiB is orders of magnitude above the
/// resident need (free-block count scale), and the heap coalesces on free.
const EARLY_HEAP_BYTES: usize = 1024 * 1024;

/// Set while inside a frame-allocator critical section. Read with `Acquire`,
/// written with `Release`; a stale read merely routes a small allocation to
/// the early heap, which frees route back out of by address — always sound.
pub(crate) static IN_FRAME_ALLOC: AtomicBool = AtomicBool::new(false);

/// Early-heap address window, published once by [`early_init`].
static EARLY_START: AtomicUsize = AtomicUsize::new(0);
static EARLY_END: AtomicUsize = AtomicUsize::new(0);

/// RAII marker held across frame-allocator index mutations.
pub(crate) struct FrameAllocGuard;

impl FrameAllocGuard {
    /// Marks entry into a frame-allocator critical section.
    pub(crate) fn enter() -> Self {
        IN_FRAME_ALLOC.store(true, Ordering::Release);
        Self
    }
}

impl Drop for FrameAllocGuard {
    fn drop(&mut self) {
        IN_FRAME_ALLOC.store(false, Ordering::Release);
    }
}

/// Backing store for the early heap. Never touched by frames.
static mut EARLY_HEAP_MEM: [u8; EARLY_HEAP_BYTES] = [0; EARLY_HEAP_BYTES];

/// Returns true if `ptr` was issued by the early heap.
#[inline(always)]
fn early_contains(ptr: *mut u8) -> bool {
    let start = EARLY_START.load(Ordering::Acquire);
    let addr = ptr as usize;
    start != 0 && addr >= start && addr < EARLY_END.load(Ordering::Acquire)
}

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

/// Global allocator: SLUB caches, buddy-Heap large fallback, static early heap.
pub struct SlabAllocator {
    inner: Mutex<SlabAllocatorInner>,
    heap: Mutex<Heap<HEAP_MAX_ORDER>>,
    early: Mutex<Heap<HEAP_MAX_ORDER>>,
}

impl SlabAllocator {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(SlabAllocatorInner::new()),
            heap: Mutex::new(Heap::new()),
            early: Mutex::new(Heap::new()),
        }
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

    /// Allocate a small object from the early heap (frame-re-entrant path).
    fn early_alloc(&self, layout: Layout) -> *mut u8 {
        let mut early = self.early.lock();
        match early.alloc(layout) {
            Ok(ptr) => ptr.as_ptr(),
            // No frame-backed growth here by design: this path runs inside
            // frame critical sections. Sized so exhaustion is unreachable.
            Err(_) => core::ptr::null_mut(),
        }
    }

    /// Free an object back to the early heap.
    fn early_dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut early = self.early.lock();
        if let Some(nonnull) = NonNull::new(ptr) {
            // SAFETY: Address routing guarantees `ptr`/`layout` match a prior
            // early-heap allocation that has not been freed.
            unsafe {
                early.dealloc(nonnull, layout);
            }
        }
    }

    /// Backs the fallback heap with fresh frames covering at least `min_bytes`.
    ///
    /// The fallback-heap mutex is already held by the caller; frame operations
    /// perform no heap allocation outside the guarded early heap, and the
    /// guard below keeps them there, so this cannot deadlock.
    fn grow_heap(heap: &mut Heap<HEAP_MAX_ORDER>, min_bytes: usize) -> bool {
        let pages = (min_bytes + (PAGE_SIZE as usize) - 1) / (PAGE_SIZE as usize);
        if pages > (1usize << (FRAME_BUDDY_ORDERS - 1)) {
            return false;
        }
        let order = Self::pages_to_order(pages);

        let phys = match FRAME_ALLOCATOR.lock().alloc_pages(order) {
            Some(p) => p,
            None => return false,
        };
        let size = (1usize << order) * (PAGE_SIZE as usize);
        let start = (phys.as_u64() + crate::mm::hhdm_offset()) as usize;
        let end = match start.checked_add(size) {
            Some(e) => e,
            None => {
                FRAME_ALLOCATOR.lock().free_pages(phys, order);
                return false;
            }
        };

        // SAFETY: `phys` is a freshly allocated, exclusive, HHDM-mapped frame
        // run, disjoint from every range previously handed to this heap.
        unsafe {
            heap.add_to_heap(start, end);
        }
        true
    }

    /// Allocate a large object via the fallback heap, growing it on demand.
    fn heap_alloc(&self, layout: Layout) -> *mut u8 {
        let mut heap = self.heap.lock();
        if let Ok(ptr) = heap.alloc(layout) {
            return ptr.as_ptr();
        }

        // Slow path: grow the heap from frames, then retry once.
        let mut pow = core::mem::size_of::<usize>();
        let need = layout.size().max(layout.align());
        while pow < need {
            pow = match pow.checked_mul(2) {
                Some(v) => v,
                None => return core::ptr::null_mut(),
            };
        }
        if !Self::grow_heap(&mut heap, pow.max(RESCUE_MIN_BYTES)) {
            return core::ptr::null_mut();
        }
        match heap.alloc(layout) {
            Ok(ptr) => ptr.as_ptr(),
            Err(_) => core::ptr::null_mut(),
        }
    }

    /// Free a large object back to the fallback heap.
    fn heap_dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut heap = self.heap.lock();
        if let Some(nonnull) = NonNull::new(ptr) {
            // SAFETY: The caller guarantees `ptr`/`layout` match a prior
            // successful fallback-heap allocation that has not been freed.
            unsafe {
                heap.dealloc(nonnull, layout);
            }
        }
    }
}

unsafe impl GlobalAlloc for SlabAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }

        // Frame-re-entrant small allocations go to the static early heap.
        // Large ones never originate inside frame critical sections; if the
        // flag is stale (other CPU), the normal path merely contends.
        if layout.size() <= 2048 && IN_FRAME_ALLOC.load(Ordering::Acquire) {
            return self.early_alloc(layout);
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

        // Large allocations (> 2048 bytes): buddy-Heap fallback
        self.heap_alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() || layout.size() == 0 {
            return;
        }

        // Address-routed first: early-heap objects return to the early heap
        // regardless of which path issued the call.
        if early_contains(ptr) {
            self.early_dealloc(ptr, layout);
            return;
        }

        let target_size = core::cmp::max(layout.size(), layout.align());

        // Small allocations: return to SLUB cache
        if target_size <= 2048 {
            if let Some(idx) = Self::class_index(target_size) {
                let mut inner = self.inner.lock();
                unsafe { inner.classes[idx].free(ptr) };
                return;
            }
        }

        // Large allocations: return to the buddy-Heap fallback
        self.heap_dealloc(ptr, layout);
    }
}

#[global_allocator]
pub static ALLOCATOR: SlabAllocator = SlabAllocator::new();

/// Arms the static early heap. Must run before any frame-allocator traffic;
/// performs no allocation itself.
pub fn early_init() {
    // Called once from `alloc::init` before any heap traffic exists.
    // `addr_of_mut!` forms a raw pointer without a mutable reference.
    let start = core::ptr::addr_of_mut!(EARLY_HEAP_MEM) as *mut u8 as usize;
    let size = EARLY_HEAP_BYTES;
    {
        let mut early = ALLOCATOR.early.lock();
        // SAFETY: Static exclusive buffer, disjoint from every managed range.
        unsafe {
            early.init(start, size);
        }
    }
    EARLY_START.store(start, Ordering::Release);
    EARLY_END.store(start + size, Ordering::Release);
}

/// Pre-seeds the fallback heap with an initial frame-backed window.
///
/// Best effort: if it fails, the rescue path in `alloc` retries on demand.
pub fn init() {
    let mut heap = ALLOCATOR.heap.lock();
    let _ = SlabAllocator::grow_heap(&mut heap, RESCUE_MIN_BYTES);
}
