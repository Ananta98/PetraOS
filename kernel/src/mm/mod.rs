pub mod address;
pub mod buddy;
pub mod freelist;
pub mod paging;
pub mod pmm;
pub mod slab;
pub mod vma;

pub use address::{PhysAddr, VirtAddr};
pub use paging::{MapError, MapFlags, PageTable, UnmapError};
pub use pmm::PMM;
pub use vma::{AddrSpace, AddrSpaceError, VmArea, VmAreaKind};

pub fn init() {
    PMM.init();
}

pub fn hhdm_offset() -> u64 {
    crate::limine::HHDM_REQUEST
        .get_response()
        .expect("Limine HHDM response is missing")
        .offset()
}
