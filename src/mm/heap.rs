use core::{alloc::Layout, panic};
use super::allocator::{FixedSizedAllocator, Locked};
use x86_64::{
    VirtAddr, 
    structures::paging::{
        FrameAllocator, 
        Mapper, 
        Page, 
        PageTableFlags, 
        Size4KiB,
        mapper::MapToError
    }
};

pub const HEAP_START: usize = 0o_000_001_000_000_0000;
pub const HEAP_SIZE: usize = 1024 * 1024; 

#[global_allocator]
static ALLOCATOR : Locked<FixedSizedAllocator> = Locked::new(FixedSizedAllocator::new());

#[alloc_error_handler]
fn alloc_error_handler(layout : Layout) -> ! {
    panic!("Allocation Error : {:?}",layout);
}

pub fn initialize_heap(mapper : &mut impl Mapper<Size4KiB>, 
                       frame_allocator : &mut impl FrameAllocator<Size4KiB>) -> Result<(), MapToError<Size4KiB>> {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator
                            .allocate_frame()
                            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe  {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush()
        };
    }

    unsafe {
        ALLOCATOR.lock().initialize(HEAP_START, HEAP_SIZE);
    }

    Ok(())
}