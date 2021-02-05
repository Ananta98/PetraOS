use spin::Mutex;
use bitvec::prelude::*;
use lazy_static::lazy_static;
use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use x86_64::{PhysAddr, structures::paging::{FrameAllocator, FrameDeallocator, PageSize, PhysFrame, Size4KiB}};

const FRAMES_BITMAP_SIZE: usize = 134082560 / (Size4KiB::SIZE as usize) / 8 + 1; 

pub struct BitmapFrameAllocator {
    memory_map : &'static mut BitSlice<Lsb0,u8>,
}

impl BitmapFrameAllocator {
    pub fn new(memory_map : &'static mut [u8]) -> Self {
        BitmapFrameAllocator {
            memory_map : BitSlice::from_slice_mut(memory_map).unwrap(),
        }
    }

    pub fn install_memory_region(&mut self,memory_map : &MemoryMap) {
        for region in memory_map.iter() {
            if region.region_type == MemoryRegionType::Usable {
                let start_address = region.range.start_addr() / Size4KiB::SIZE;
                let end_address = region.range.end_addr() / Size4KiB::SIZE;
                for index in start_address..end_address {
                    self.memory_map.set(index as usize, true);
                }
            }
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for BitmapFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.memory_map
                    .iter()
                    .enumerate()
                    .filter_map(|(index,unused)| {
                        if *unused {
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
        static mut MAP: [u8; FRAMES_BITMAP_SIZE] = [0; FRAMES_BITMAP_SIZE];
        Mutex::new(BitmapFrameAllocator::new(unsafe { &mut MAP }))
    };
}