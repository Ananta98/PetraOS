use x86_64::PhysAddr;
use bootloader::bootinfo::MemoryMap;
use bootloader::bootinfo::MemoryRegionType;
use x86_64::structures::paging::{FrameAllocator, Size4KiB, PhysFrame};

pub struct BootInfoFrameAllocator {
    memory_map : &'static MemoryMap,
    next : usize
}

impl BootInfoFrameAllocator {
    pub fn initialize(memory_map : &'static MemoryMap) -> Self {
        BootInfoFrameAllocator {
            memory_map : memory_map,
            next : 0,
        }
    }
    
    pub fn find_usable_region(&self) -> impl Iterator<Item=PhysFrame> {
        let regions = self.memory_map.iter();
        let usable_regions = regions.filter(|r| r.region_type == MemoryRegionType::Usable);
        let address_ranges = usable_regions.map(|r| r.range.start_addr()..r.range.end_addr());
        let frame_addresses = address_ranges.flat_map(|r| r.step_by(4096));
        frame_addresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let usable_frame = self.find_usable_region().nth(self.next);
        self.next += 1;   
        usable_frame
    }
}