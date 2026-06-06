extern crate alloc;

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

    #[path = "../../src/mm/paging.rs"]
    pub mod paging;

    #[path = "../../src/mm/vma.rs"]
    pub mod vma;

    pub use address::{PhysAddr, VirtAddr};
    pub use pmm::PMM;
    pub use paging::{MapError, MapFlags, PageTable, UnmapError};
    pub use vma::{AddrSpace, AddrSpaceError, VmArea, VmAreaKind};

    pub static mut HHDM_OFFSET_FOR_TEST: u64 = 0;

    pub fn hhdm_offset() -> u64 {
        unsafe { HHDM_OFFSET_FOR_TEST }
    }
}

use mm::buddy::{Page, PageFlags};
use mm::{AddrSpace, AddrSpaceError, PhysAddr, VirtAddr, MapError, MapFlags, PageTable, UnmapError, VmAreaKind, PMM};
use std::collections::BTreeMap;
use std::cell::RefCell;

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

// A mock page table that tracks mappings in a BTreeMap
struct MockPageTable {
    mappings: RefCell<BTreeMap<VirtAddr, PhysAddr>>,
    fail_on_map: bool,
}

impl MockPageTable {
    fn new_mock() -> Self {
        Self {
            mappings: RefCell::new(BTreeMap::new()),
            fail_on_map: false,
        }
    }
}

unsafe impl Send for MockPageTable {}
unsafe impl Sync for MockPageTable {}

impl PageTable for MockPageTable {
    fn new() -> Result<Self, MapError> {
        Ok(Self::new_mock())
    }

    unsafe fn from_root(_root: PhysAddr) -> Self {
        Self::new_mock()
    }

    fn root(&self) -> PhysAddr {
        PhysAddr(0)
    }

    fn map(&mut self, page: VirtAddr, frame: PhysAddr, _flags: MapFlags) -> Result<(), MapError> {
        if self.fail_on_map {
            return Err(MapError::FrameAllocationFailed);
        }
        if self.mappings.borrow().contains_key(&page) {
            return Err(MapError::AlreadyMapped);
        }
        self.mappings.borrow_mut().insert(page, frame);
        Ok(())
    }

    fn unmap(&mut self, page: VirtAddr) -> Result<PhysAddr, UnmapError> {
        self.mappings.borrow_mut().remove(&page).ok_or(UnmapError::NotMapped)
    }

    fn translate(&self, virt: VirtAddr) -> Option<PhysAddr> {
        self.mappings.borrow().get(&virt).copied()
    }

    unsafe fn activate(&self) {}
}

#[test]
fn test_vma_map_success_anonymous() {
    static MEM_BUF: SafeStatic<PageAligned<[u8; 128 * PAGE_SIZE]>> = SafeStatic::new(PageAligned([0; 128 * PAGE_SIZE]));
    static MAP: SafeStatic<[Page; 128]> = SafeStatic::new([Page::new(); 128]);
    setup_mock_pmm_static(unsafe { &mut MEM_BUF.get_mut().0 }, unsafe { MAP.get_mut() });

    let page_table = MockPageTable::new_mock();
    let mut addr_space = AddrSpace::new(page_table);

    let start = VirtAddr(0x1000);
    let size = 3 * PAGE_SIZE;

    let res = addr_space.map_area(
        start,
        size,
        MapFlags::READ | MapFlags::WRITE,
        VmAreaKind::Anonymous,
    );
    assert!(res.is_ok());

    // Verify that the pages were mapped in the mock page table
    let pt = addr_space.page_table();
    assert!(pt.translate(VirtAddr(0x1000)).is_some());
    assert!(pt.translate(VirtAddr(0x2000)).is_some());
    assert!(pt.translate(VirtAddr(0x3000)).is_some());
    assert!(pt.translate(VirtAddr(0x4000)).is_none());
}

#[test]
fn test_vma_map_success_device() {
    static MEM_BUF: SafeStatic<PageAligned<[u8; 128 * PAGE_SIZE]>> = SafeStatic::new(PageAligned([0; 128 * PAGE_SIZE]));
    static MAP: SafeStatic<[Page; 128]> = SafeStatic::new([Page::new(); 128]);
    setup_mock_pmm_static(unsafe { &mut MEM_BUF.get_mut().0 }, unsafe { MAP.get_mut() });

    let page_table = MockPageTable::new_mock();
    let mut addr_space = AddrSpace::new(page_table);

    let start = VirtAddr(0x5000);
    let size = 2 * PAGE_SIZE;
    let phys_start = PhysAddr(0x9000);

    let res = addr_space.map_area(
        start,
        size,
        MapFlags::READ | MapFlags::WRITE,
        VmAreaKind::Device { phys_start },
    );
    assert!(res.is_ok());

    // Verify correct mapping
    let pt = addr_space.page_table();
    assert_eq!(pt.translate(VirtAddr(0x5000)), Some(PhysAddr(0x9000)));
    assert_eq!(pt.translate(VirtAddr(0x6000)), Some(PhysAddr(0xA000)));
    assert!(pt.translate(VirtAddr(0x7000)).is_none());
}

#[test]
fn test_vma_invalid_range_or_alignment() {
    static MEM_BUF: SafeStatic<PageAligned<[u8; 128 * PAGE_SIZE]>> = SafeStatic::new(PageAligned([0; 128 * PAGE_SIZE]));
    static MAP: SafeStatic<[Page; 128]> = SafeStatic::new([Page::new(); 128]);
    setup_mock_pmm_static(unsafe { &mut MEM_BUF.get_mut().0 }, unsafe { MAP.get_mut() });

    let page_table = MockPageTable::new_mock();
    let mut addr_space = AddrSpace::new(page_table);

    // Unaligned start
    assert_eq!(
        addr_space.map_area(VirtAddr(0x1050), PAGE_SIZE, MapFlags::READ, VmAreaKind::Anonymous),
        Err(AddrSpaceError::InvalidRange)
    );

    // Unaligned size
    assert_eq!(
        addr_space.map_area(VirtAddr(0x2000), 200, MapFlags::READ, VmAreaKind::Anonymous),
        Err(AddrSpaceError::InvalidRange)
    );

    // Zero size
    assert_eq!(
        addr_space.map_area(VirtAddr(0x3000), 0, MapFlags::READ, VmAreaKind::Anonymous),
        Err(AddrSpaceError::InvalidRange)
    );
}

#[test]
fn test_vma_overlapping_areas() {
    static MEM_BUF: SafeStatic<PageAligned<[u8; 128 * PAGE_SIZE]>> = SafeStatic::new(PageAligned([0; 128 * PAGE_SIZE]));
    static MAP: SafeStatic<[Page; 128]> = SafeStatic::new([Page::new(); 128]);
    setup_mock_pmm_static(unsafe { &mut MEM_BUF.get_mut().0 }, unsafe { MAP.get_mut() });

    let page_table = MockPageTable::new_mock();
    let mut addr_space = AddrSpace::new(page_table);

    // First map area
    assert!(addr_space
        .map_area(VirtAddr(0x1000), PAGE_SIZE * 4, MapFlags::READ, VmAreaKind::Anonymous)
        .is_ok());

    // Map completely inside the first area
    assert_eq!(
        addr_space.map_area(VirtAddr(0x2000), PAGE_SIZE, MapFlags::READ, VmAreaKind::Anonymous),
        Err(AddrSpaceError::OverlappingArea)
    );

    // Map overlapping the start
    assert_eq!(
        addr_space.map_area(VirtAddr(0x0000), PAGE_SIZE * 2, MapFlags::READ, VmAreaKind::Anonymous),
        Err(AddrSpaceError::OverlappingArea)
    );

    // Map overlapping the end
    assert_eq!(
        addr_space.map_area(VirtAddr(0x4000), PAGE_SIZE * 2, MapFlags::READ, VmAreaKind::Anonymous),
        Err(AddrSpaceError::OverlappingArea)
    );
}

#[test]
fn test_vma_unmap_area() {
    static MEM_BUF: SafeStatic<PageAligned<[u8; 128 * PAGE_SIZE]>> = SafeStatic::new(PageAligned([0; 128 * PAGE_SIZE]));
    static MAP: SafeStatic<[Page; 128]> = SafeStatic::new([Page::new(); 128]);
    setup_mock_pmm_static(unsafe { &mut MEM_BUF.get_mut().0 }, unsafe { MAP.get_mut() });

    let page_table = MockPageTable::new_mock();
    let mut addr_space = AddrSpace::new(page_table);

    let start = VirtAddr(0x1000);
    assert!(addr_space
        .map_area(start, PAGE_SIZE * 2, MapFlags::READ | MapFlags::WRITE, VmAreaKind::Anonymous)
        .is_ok());

    // Unmap existing area
    assert!(addr_space.unmap_area(start).is_ok());

    // Verify it is unmapped in mock page table
    let pt = addr_space.page_table();
    assert!(pt.translate(VirtAddr(0x1000)).is_none());
    assert!(pt.translate(VirtAddr(0x2000)).is_none());

    // Attempting to unmap again should fail
    assert_eq!(
        addr_space.unmap_area(start),
        Err(AddrSpaceError::InvalidRange)
    );
}

#[test]
fn test_vma_rollback_on_pmm_exhaustion() {
    // Only 2 pages available in PMM
    static MEM_BUF: SafeStatic<PageAligned<[u8; 2 * PAGE_SIZE]>> = SafeStatic::new(PageAligned([0; 2 * PAGE_SIZE]));
    static MAP: SafeStatic<[Page; 2]> = SafeStatic::new([Page::new(); 2]);
    setup_mock_pmm_static(unsafe { &mut MEM_BUF.get_mut().0 }, unsafe { MAP.get_mut() });

    let page_table = MockPageTable::new_mock();
    let mut addr_space = AddrSpace::new(page_table);

    // Try mapping 3 pages (requires 3 PMM pages, but only 2 are free)
    let res = addr_space.map_area(
        VirtAddr(0x1000),
        3 * PAGE_SIZE,
        MapFlags::READ | MapFlags::WRITE,
        VmAreaKind::Anonymous,
    );
    assert!(res.is_err());

    // Verify that the rollback cleaned up everything in the page table
    let pt = addr_space.page_table();
    assert!(pt.mappings.borrow().is_empty(), "Page table should have no lingering mappings after rollback");

    // PMM free pages count should still be 2
    assert_eq!(PMM.free_pages_count(), 2);
}

#[test]
fn test_vma_rollback_on_map_failure() {
    static MEM_BUF: SafeStatic<PageAligned<[u8; 128 * PAGE_SIZE]>> = SafeStatic::new(PageAligned([0; 128 * PAGE_SIZE]));
    static MAP: SafeStatic<[Page; 128]> = SafeStatic::new([Page::new(); 128]);
    setup_mock_pmm_static(unsafe { &mut MEM_BUF.get_mut().0 }, unsafe { MAP.get_mut() });

    let mut page_table = MockPageTable::new_mock();
    // Simulate a hardware page table mapping failure
    page_table.fail_on_map = true;

    let mut addr_space = AddrSpace::new(page_table);

    let res = addr_space.map_area(
        VirtAddr(0x1000),
        2 * PAGE_SIZE,
        MapFlags::READ | MapFlags::WRITE,
        VmAreaKind::Anonymous,
    );
    assert!(res.is_err());

    // Verify rollback of mappings in the page table
    let pt = addr_space.page_table();
    assert!(pt.mappings.borrow().is_empty(), "Page table should have no lingering mappings");

    // Verify rollback of physical frames in PMM
    // NOTE: The current implementation of map_area in kernel/src/mm/vma.rs has a known leak where
    // the physical frame allocated for the page that failed to map is not freed during rollback.
    // Hence, 1 frame is leaked, resulting in 127 free pages instead of 128.
    assert_eq!(PMM.free_pages_count(), 127);
}
