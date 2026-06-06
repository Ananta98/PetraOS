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

    pub use address::{PhysAddr, VirtAddr};
    pub use pmm::PMM;

    pub static mut HHDM_OFFSET_FOR_TEST: u64 = 0;

    pub fn hhdm_offset() -> u64 {
        unsafe { HHDM_OFFSET_FOR_TEST }
    }
}

use mm::buddy::{Page, PageFlags};
use mm::{PhysAddr, PMM};

const PAGE_SIZE: usize = 4096;

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

/// Set up a mock physical memory area and initialize PMM for testing using static buffers.
fn setup_mock_pmm_static(
    memory_buf: &'static mut [u8],
    page_map: &'static mut [Page],
) {
    let hhdm_offset = memory_buf.as_ptr() as u64;
    let num_pages = page_map.len();

    // Mark all pages as usable
    for page in page_map.iter_mut() {
        *page = Page::new();
        page.flags.insert(PageFlags::USABLE);
    }

    // Construct the BuddyAllocator
    let mut allocator = mm::buddy::BuddyAllocator::new(page_map, hhdm_offset, num_pages);

    // Free the entire region initially to populate free lists
    allocator.free_initial_region(0, num_pages);

    // Set the global test variables
    unsafe {
        mm::HHDM_OFFSET_FOR_TEST = hhdm_offset;
    }

    // Initialize PMM with this allocator
    PMM.init_for_test(allocator);
}

#[test]
fn test_basic_allocation_and_free() {
    static MEM_BUF: SafeStatic<[u8; 128 * PAGE_SIZE]> = SafeStatic::new([0; 128 * PAGE_SIZE]);
    static MAP: SafeStatic<[Page; 128]> = SafeStatic::new([Page::new(); 128]);
    setup_mock_pmm_static(unsafe { MEM_BUF.get_mut() }, unsafe { MAP.get_mut() });
    let initial_free = PMM.free_pages_count();

    // Allocate a single page
    let paddr = PMM.alloc_page().expect("Failed to allocate a page");
    assert!(paddr.is_aligned(PAGE_SIZE as u64));
    assert_eq!(PMM.free_pages_count(), initial_free - 1);

    // Write data to the page using direct physical-to-virtual address mapping
    unsafe {
        let virt_ptr = (paddr.as_u64() + mm::HHDM_OFFSET_FOR_TEST) as *mut u64;
        virt_ptr.write(0xDEADBEEFCAFEBABE);
        assert_eq!(virt_ptr.read(), 0xDEADBEEFCAFEBABE);
    }

    // Free the page
    PMM.free_page(paddr);
    assert_eq!(PMM.free_pages_count(), initial_free);
}

#[test]
fn test_multipage_allocation_and_alignment() {
    static MEM_BUF: SafeStatic<[u8; 256 * PAGE_SIZE]> = SafeStatic::new([0; 256 * PAGE_SIZE]);
    static MAP: SafeStatic<[Page; 256]> = SafeStatic::new([Page::new(); 256]);
    setup_mock_pmm_static(unsafe { MEM_BUF.get_mut() }, unsafe { MAP.get_mut() });
    
    // Allocate order 3 (8 pages = 32KB)
    let order = 3;
    let pages = 1 << order;
    let initial_free = PMM.free_pages_count();

    let paddr = PMM.alloc_pages(order).expect("Failed to allocate 8 contiguous pages");
    
    // Address must be aligned to the block size: (2^order * PAGE_SIZE)
    let block_size = (pages * PAGE_SIZE) as u64;
    assert!(paddr.is_aligned(block_size), "Address {:?} is not aligned to {}", paddr, block_size);
    assert_eq!(PMM.free_pages_count(), initial_free - pages);

    // Free the block
    PMM.free_pages(paddr, order);
    assert_eq!(PMM.free_pages_count(), initial_free);
}

#[test]
fn test_splitting_and_merging() {
    static MEM_BUF: SafeStatic<[u8; 128 * PAGE_SIZE]> = SafeStatic::new([0; 128 * PAGE_SIZE]);
    static MAP: SafeStatic<[Page; 128]> = SafeStatic::new([Page::new(); 128]);
    setup_mock_pmm_static(unsafe { MEM_BUF.get_mut() }, unsafe { MAP.get_mut() });
    let initial_free = PMM.free_pages_count();

    // Allocate one order-0 page. This should cause a larger block to split.
    let paddr1 = PMM.alloc_page().expect("Failed to allocate page 1");
    assert_eq!(PMM.free_pages_count(), initial_free - 1);

    // Allocate another order-0 page. It might be adjacent (its buddy).
    let paddr2 = PMM.alloc_page().expect("Failed to allocate page 2");
    assert_eq!(PMM.free_pages_count(), initial_free - 2);

    // Free them both, which should trigger recursive buddy merging back to original state.
    PMM.free_page(paddr1);
    PMM.free_page(paddr2);
    assert_eq!(PMM.free_pages_count(), initial_free);
}

#[test]
fn test_out_of_memory() {
    static MEM_BUF: SafeStatic<[u8; 16 * PAGE_SIZE]> = SafeStatic::new([0; 16 * PAGE_SIZE]);
    static MAP: SafeStatic<[Page; 16]> = SafeStatic::new([Page::new(); 16]);
    setup_mock_pmm_static(unsafe { MEM_BUF.get_mut() }, unsafe { MAP.get_mut() });

    // Try allocating more pages than we have (e.g. order 5 = 32 pages)
    let paddr_too_large = PMM.alloc_pages(5);
    assert!(paddr_too_large.is_none(), "Should fail to allocate more memory than total capacity");

    // Exhaust all 16 pages by allocating them one by one
    let mut allocated = [PhysAddr(0); 16];
    for i in 0..16 {
        let paddr = PMM.alloc_page();
        assert!(paddr.is_some(), "Should be able to allocate up to 16 pages");
        allocated[i] = paddr.unwrap();
    }

    // Now PMM should be completely out of memory
    assert_eq!(PMM.free_pages_count(), 0);
    assert!(PMM.alloc_page().is_none(), "Should fail allocation when out of memory");

    // Free everything back
    for &paddr in &allocated {
        PMM.free_page(paddr);
    }
    assert_eq!(PMM.free_pages_count(), 16);
}

#[test]
fn test_double_free_protection() {
    static MEM_BUF: SafeStatic<[u8; 16 * PAGE_SIZE]> = SafeStatic::new([0; 16 * PAGE_SIZE]);
    static MAP: SafeStatic<[Page; 16]> = SafeStatic::new([Page::new(); 16]);
    setup_mock_pmm_static(unsafe { MEM_BUF.get_mut() }, unsafe { MAP.get_mut() });
    let initial_free = PMM.free_pages_count();

    let paddr = PMM.alloc_page().expect("Failed to allocate initial page");
    assert_eq!(PMM.free_pages_count(), initial_free - 1);

    // First free: correct
    PMM.free_page(paddr);
    assert_eq!(PMM.free_pages_count(), initial_free);

    // Second free (double free): PMM should log an error and safely return/noop without crashing or double-merging.
    PMM.free_page(paddr);
    // Free count should remain initial_free (not increase further)
    assert_eq!(PMM.free_pages_count(), initial_free);
}

#[test]
fn test_invalid_free_protection() {
    static MEM_BUF: SafeStatic<[u8; 16 * PAGE_SIZE]> = SafeStatic::new([0; 16 * PAGE_SIZE]);
    static MAP: SafeStatic<[Page; 16]> = SafeStatic::new([Page::new(); 16]);
    setup_mock_pmm_static(unsafe { MEM_BUF.get_mut() }, unsafe { MAP.get_mut() });
    let initial_free = PMM.free_pages_count();

    // Free a page out of bounds (e.g. index 9999)
    let oob_paddr = PhysAddr(9999 * PAGE_SIZE as u64);
    PMM.free_page(oob_paddr);
    assert_eq!(PMM.free_pages_count(), initial_free, "Freeing OOB page should not affect free pages count");
}
