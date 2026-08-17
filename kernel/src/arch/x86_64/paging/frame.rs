use crate::mm::hhdm_offset;
use crate::mm::pmm::PMM;
use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};

/// Frame allocator for `x86_64` paging mapper, backed by the physical memory manager (PMM).
pub struct KernelFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for KernelFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let frame_phys = PMM.alloc_page()?;
        let hhdm = hhdm_offset();
        // Intermediate page tables allocated by OffsetPageTable must be zeroed.
        unsafe {
            let ptr = (frame_phys.as_u64() + hhdm) as *mut u8;
            core::ptr::write_bytes(ptr, 0, 4096);
        }
        PhysFrame::from_start_address(frame_phys).ok()
    }
}
