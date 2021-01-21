#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

mod driver;
mod arch;

use core::panic::PanicInfo;
use arch::idt::initialize_idt;
use arch::gdt::initialize_gdt;

#[panic_handler]
fn panic(_info : &PanicInfo) -> ! {
    println!("{}",_info);
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    initialize_gdt();
    initialize_idt();
    x86_64::instructions::interrupts::enable();
    unsafe {
        *(0xdeadbeef as *mut u64) = 42;
    };
    loop {}
}   
