#[path = "../../src/limine.rs"]
pub mod limine;

#[path = "."]
pub mod sync {
    #[path = "."]
    pub mod spinlock {
        #[path = "../../src/sync/spinlock.rs"]
        pub mod impl_spinlock;
        pub use impl_spinlock::Spinlock;
    }
}

#[path = "."]
pub mod mm {
    #[path = "../../src/mm/address.rs"]
    pub mod address;
    
    #[path = "../../src/mm/freelist.rs"]
    pub mod freelist;
    
    #[path = "../../src/mm/buddy.rs"]
    pub mod buddy;
    
    #[path = "../../src/mm/pmm.rs"]
    pub mod pmm;

    #[path = "../../src/mm/slab.rs"]
    pub mod slab;

    pub use address::{PhysAddr, VirtAddr};
    pub use pmm::PMM;
    pub use slab::{KmemCache, SlabAllocator};

    pub static mut HHDM_OFFSET_FOR_TEST: u64 = 0;

    pub fn hhdm_offset() -> u64 {
        unsafe { HHDM_OFFSET_FOR_TEST }
    }
}

use mm::buddy::{Page, PageFlags};
use mm::{KmemCache, SlabAllocator, PMM};
use core::alloc::{GlobalAlloc, Layout};

const PAGE_SIZE: usize = 4096;

#[repr(align(4096))]
struct PageAligned<T>(T);

struct SafeStatic<T>(core::cell::UnsafeCell<T>);
unsafe impl<T> Sync for SafeStatic<T> {}

impl<T> SafeStatic<T> {
    const fn new(val: T) -> Self {
        Self(core::cell::UnsafeCell::new(val))
    }
    
    unsafe fn get_mut(&self) -> &'static mut T {
        unsafe { &mut *self.0.get() }
    }
}

#[repr(C)]
struct MockHhdmResponse {
    revision: u64,
    offset: u64,
}

static mut MOCK_RESPONSE: MockHhdmResponse = MockHhdmResponse {
    revision: 0,
    offset: 0,
};

#[used]
#[unsafe(link_section = ".init_array")]
static INIT_HEAP_TEST: unsafe extern "C" fn() = init_heap_test;

unsafe extern "C" fn init_heap_test() {
    // 1. Initialize HhdmResponse pointer in HHDM_REQUEST response field
    unsafe {
        let req_ptr = &limine::HHDM_REQUEST as *const _ as *mut u8;
        let response_ptr = req_ptr.add(40) as *mut Option<core::ptr::NonNull<MockHhdmResponse>>;
        *response_ptr = Some(core::ptr::NonNull::new_unchecked(&raw mut MOCK_RESPONSE));
    }

    // 2. Initialize PMM with 256 pages statically using SafeStatic
    static MEM_BUF: SafeStatic<PageAligned<[u8; 256 * PAGE_SIZE]>> = SafeStatic::new(PageAligned([0; 256 * PAGE_SIZE]));
    static MAP: SafeStatic<[Page; 256]> = SafeStatic::new([Page::new(); 256]);

    let hhdm_offset = unsafe { MEM_BUF.get_mut().0.as_ptr() as u64 };
    unsafe {
        MOCK_RESPONSE.offset = hhdm_offset;
        mm::HHDM_OFFSET_FOR_TEST = hhdm_offset;
    }

    let page_map = unsafe { MAP.get_mut() };

    // Mark pages usable
    for page in page_map.iter_mut() {
        *page = Page::new();
        page.flags.insert(PageFlags::USABLE);
    }

    let mut allocator = mm::buddy::BuddyAllocator::new(page_map, hhdm_offset, 256);
    allocator.free_initial_region(0, 256);

    PMM.init_for_test(allocator);
}

#[test]
fn test_kmem_cache_basic_alloc_free() {
    let hhdm = unsafe { mm::HHDM_OFFSET_FOR_TEST };

    // Create a cache for 32-byte objects aligned to 32 bytes
    let mut cache = KmemCache::new("test-cache-32", 32, 32);

    // Allocate an object
    let obj1 = unsafe { cache.alloc(hhdm) };
    assert!(!obj1.is_null());
    assert_eq!(obj1 as usize % 32, 0, "Object must be 32-byte aligned");

    // Write some data to verify it is writable
    unsafe {
        *(obj1 as *mut u64) = 0xABCDEF;
        assert_eq!(*(obj1 as *mut u64), 0xABCDEF);
    }

    // Allocate another object
    let obj2 = unsafe { cache.alloc(hhdm) };
    assert!(!obj2.is_null());
    assert_ne!(obj1, obj2, "Objects must have different addresses");

    // Free both objects
    unsafe {
        cache.free(obj1, hhdm);
        cache.free(obj2, hhdm);
    }

    // Allocate again, verifying reuse of one of the freed addresses
    let obj3 = unsafe { cache.alloc(hhdm) };
    assert!(obj3 == obj1 || obj3 == obj2, "Should reuse previously freed memory block");
    unsafe {
        cache.free(obj3, hhdm);
    }
}

#[test]
fn test_slab_allocator_small_alloc() {
    let allocator = SlabAllocator::new();

    // Allocate 16 bytes aligned to 16 bytes
    let layout = Layout::from_size_align(16, 16).unwrap();
    let ptr = unsafe { allocator.alloc(layout) };
    assert!(!ptr.is_null());
    assert_eq!(ptr as usize % 16, 0);

    // Free the block
    unsafe {
        allocator.dealloc(ptr, layout);
    }
}

#[test]
fn test_slab_allocator_large_alloc() {
    let allocator = SlabAllocator::new();
    let initial_free = PMM.free_pages_count();

    // Allocate 4096 bytes (1 page). This is > 2048 bytes, so it should bypass slab caches
    // and allocate directly from PMM.
    let layout = Layout::from_size_align(4096, 4096).unwrap();
    let ptr = unsafe { allocator.alloc(layout) };
    assert!(!ptr.is_null());
    assert_eq!(ptr as usize % 4096, 0);

    // 1 page should have been allocated directly from PMM
    assert_eq!(PMM.free_pages_count(), initial_free - 1);

    // Free the block
    unsafe {
        allocator.dealloc(ptr, layout);
    }

    // Memory should be returned to PMM
    assert_eq!(PMM.free_pages_count(), initial_free);
}

#[test]
fn test_slab_allocator_multiple_sizes() {
    let allocator = SlabAllocator::new();

    let layouts = [
        Layout::from_size_align(8, 8).unwrap(),
        Layout::from_size_align(32, 32).unwrap(),
        Layout::from_size_align(128, 128).unwrap(),
        Layout::from_size_align(1024, 1024).unwrap(),
    ];

    let mut ptrs = Vec::new();

    for &layout in &layouts {
        let ptr = unsafe { allocator.alloc(layout) };
        assert!(!ptr.is_null());
        assert_eq!(ptr as usize % layout.align(), 0);
        ptrs.push(ptr);
    }

    for (ptr, &layout) in ptrs.into_iter().zip(&layouts) {
        unsafe {
            allocator.dealloc(ptr, layout);
        }
    }
}
