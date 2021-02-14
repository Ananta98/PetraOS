use crate::println;
use spin::Mutex;
use bit_vec::BitVec;
use lazy_static::lazy_static;
use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use x86_64::{PhysAddr, structures::paging::{FrameAllocator, FrameDeallocator, PageSize, PhysFrame, Size4KiB, page}};

pub struct BitmapFrameAllocator {
    memory_map : BitVec,
}

impl BitmapFrameAllocator {
    pub fn initialize_frame_allocator(&mut self,memory_map : &MemoryMap) {
        let bitmap_size = BitmapFrameAllocator::get_highest_phys_address(memory_map) / Size4KiB::SIZE / 8;
        println!("Highest Address : {}", BitmapFrameAllocator::get_highest_phys_address(memory_map));
        self.memory_map = BitVec::from_elem(bitmap_size as usize,false);
        for region in memory_map.iter() {
            if region.region_type == MemoryRegionType::Usable {
                let start_address = region.range.start_addr() / Size4KiB::SIZE;
                let end_address = region.range.end_addr() / Size4KiB::SIZE;
                let mut page_count = (end_address - start_address) / Size4KiB::SIZE; 
                if page_count % Size4KiB::SIZE == 0 {
                    page_count += 1;
                }
                for index in start_address..start_address + page_count  {
                    self.memory_map.set(index as usize, true);
                }
            }
        }
        println!("Frame Allocator ready to use");
    }

    fn get_highest_phys_address(memory_map : &MemoryMap) -> u64 {
        memory_map
        .iter()
        .filter(|r| r.region_type == MemoryRegionType::Usable)
        .map(|r| r.range.start_addr()..r.range.end_addr())
        .flat_map(|r| r.step_by(4096))
        .max()
        .unwrap()
    }
}

unsafe impl FrameAllocator<Size4KiB> for BitmapFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.memory_map
                    .iter()
                    .enumerate()
                    .filter_map(|(index,unused)| {
                        if unused {
                            let address = PhysFrame::containing_address(PhysAddr::new(index as u64 * Size4KiB::SIZE));
                            Some((index, address))
                        } else {
                            None
                        }
                    })
                    .next();
        if let Some((index,address)) = frame {
            self.memory_map.set(index,false);
            Some(address)
        } else {
            None
        }
    }
}

impl FrameDeallocator<Size4KiB> for BitmapFrameAllocator {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
        let index = frame.start_address().as_u64() / Size4KiB::SIZE;
        self.memory_map.set(index as usize, true);
    }
}

lazy_static! {
    pub static ref FRAME_ALLOCATOR: Mutex<BitmapFrameAllocator> = {
        Mutex::new(BitmapFrameAllocator { memory_map : BitVec::new() })
    };
}