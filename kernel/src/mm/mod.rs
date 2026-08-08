pub mod alloc;
pub mod pmm;
pub mod types;
pub mod vmm;

pub use pmm::PMM;
pub use types::{PhysAddr, VirtAddr};
pub use vmm::{
    AddrSpace, AddrSpaceError, MapError, MapFlags, PageFaultAccess, PageFaultError, PageTable,
    UnmapError, VmArea, VmAreaKind,
};

pub fn init() {
    PMM.init();

    // Verify heap allocator mapping works
    extern crate alloc;
    let heap_test = alloc::boxed::Box::new(42);
    assert_eq!(*heap_test, 42);
}

pub fn hhdm_offset() -> u64 {
    crate::limine::HHDM_REQUEST
        .get_response()
        .expect("Limine HHDM response is missing")
        .offset()
}
