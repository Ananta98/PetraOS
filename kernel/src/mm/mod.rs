pub mod alloc;
pub mod dma;
pub mod pmm;
pub mod vmm;

pub use pmm::PMM;
pub use vmm::{AddrSpace, AddrSpaceError, COW_FLAG, PageFaultError, PageTable, VmArea, VmAreaKind};
pub use crate::arch::paging::{
    ArchPageTable, active_cr3, ensure_mapped, map_mmio, read_cr2,
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
