use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use x86_64::{PhysAddr, structures::paging::{FrameAllocator, FrameDeallocator, PageSize, PhysFrame, Size4KiB}};

pub struct BootInfoFrameAllocator {
    memory_map : &'static MemoryMap,
    next : usize,
}

impl BootInfoFrameAllocator {
   pub fn initialize_frame_allocator(memory_map : &'static MemoryMap) -> Self {
       BootInfoFrameAllocator {
           memory_map : memory_map,
           next : 0,
       }
   }

   fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
       let regions = self.memory_map.iter();
       let usable_regions = regions.filter(|r| r.region_type == MemoryRegionType::Usable);
       let addr_ranges = usable_regions.map(|r| r.range.start_addr()..r.range.end_addr());
       let frame_adresses = addr_ranges.flat_map(|r| r.step_by(4096));
       frame_adresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
   }

}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}

impl FrameDeallocator<Size4KiB> for BootInfoFrameAllocator{
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
        let index = frame.start_address().as_u64() / Size4KiB::SIZE;
        if index == 0 {
            self.next = 0;
        } else {
            self.next = index as usize - 1;
        }
    }
}