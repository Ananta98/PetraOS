#![no_std]
#![no_main]
#![allow(dead_code)]
#![feature(const_fn)]
#![feature(const_mut_refs)]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(const_in_array_repeat_expressions)]

extern crate alloc;

mod arch;
mod mm;

use core::{panic::PanicInfo};
use bootloader::{BootInfo, entry_point};
use x86_64::VirtAddr;
use mm::heap;

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
    let mut mapper = unsafe { mm::initialize_paging(phys_mem_offset)};
    heap::initialize_heap(&mut mapper,&mut boot_info.memory_map).expect("Heap initialization failed");
    hlt_loop();
}