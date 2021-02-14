#![no_std]
#![no_main]
#![allow(dead_code)]
#![feature(const_fn)]
#![feature(const_mut_refs)]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(const_in_array_repeat_expressions)]
#![feature(wake_trait)]

extern crate alloc;

mod arch;
mod mm;
mod task;

use core::{panic::PanicInfo};
use arch::pit::PIT;
use bootloader::{BootInfo, entry_point};
use drivers::ata::ATA;
use task::executor::Executor;
use x86_64::VirtAddr;
use mm::{heap_allocator,boot_frame, frame_allocator::*};
use task::Task;
use crate::drivers::keyboard::keyboard_pressed;

#[macro_use]
mod drivers;

fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("PANIC : {}", info);
    hlt_loop();
}

entry_point!(kernel_main);

fn kernel_main(boot_info : &'static BootInfo) -> ! {
    arch::gdt_tss::initialize();
    arch::interrupts::initialize_idt();
    unsafe { arch::interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { arch::paging::initialize_paging(phys_mem_offset)};
    let mut boot_frame_allocator = boot_frame::BootInfoFrameAllocator::initialize(&boot_info.memory_map);
    let mut pit = PIT::new();
    pit.initialize(2685);
    heap_allocator::initialize_heap(&mut mapper,&mut boot_frame_allocator).expect("Heap initialization failed");
    FRAME_ALLOCATOR.lock().initialize_frame_allocator(&boot_info.memory_map);
    let mut primary_ata = ATA::new(0x1F0);
    // let buf : [u8; 10] = [1,2,3,4,5,6,7,8,9,10];
    primary_ata.identify();
    // primary_ata.write_all_sectors(0, &buf, 10);
    // let mut temp_buf : [u8; 10] = [0; 10];
    // primary_ata.read_all_sectors(0, &mut temp_buf, 10);
    // println!("{:?}",temp_buf);
    
    let mut executor = Executor::new();
    executor.spawn(Task::new(keyboard_pressed()));
    executor.run();
    
}