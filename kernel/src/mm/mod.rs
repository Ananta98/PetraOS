pub mod alloc;
pub mod dma;
pub mod user;
pub mod vmm;

pub use alloc::{FRAME_ALLOCATOR, PMM};
pub use user::{UserCStr, UserPtr, USER_SPACE_MAX_ADDR};
pub use vmm::{
    AddrSpace, AddrSpaceError, COW_FLAG, PageFaultError, PageFaultErrorCode, PageTable,
    PageTableEntry, PageTableFlags, PagingError, PhysAddr, VirtAddr, VmArea, VmAreaKind,
};
pub use crate::arch::paging::{
    ArchPageTable, active_cr3, ensure_mapped, map_mmio, read_cr2,
};

pub fn init() {
    alloc::init();
}

pub fn hhdm_offset() -> u64 {
    crate::limine::HHDM_REQUEST
        .get_response()
        .expect("Limine HHDM response is missing")
        .offset()
}
