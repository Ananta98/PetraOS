pub mod alloc;
pub mod pmm;
pub mod types;
pub mod vmm;

pub use pmm::PMM;
pub use types::{PhysAddr, VirtAddr, VmAreaKind};
pub use vmm::{
    AddrSpace, AddrSpaceError, MapError, MapFlags, PageFaultAccess, PageFaultError, PageTable,
    UnmapError, VmArea,
};


pub fn init() {
    PMM.init();
}

pub fn hhdm_offset() -> u64 {
    crate::limine::HHDM_REQUEST
        .get_response()
        .expect("Limine HHDM response is missing")
        .offset()
}
