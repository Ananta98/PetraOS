#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

#[cfg_attr(target_arch = "x86_64", path = "arch/x86_64/mod.rs")]
mod arch;

pub mod device;
pub mod drivers;
pub mod fs;
pub mod limine;
pub mod logger;
pub mod mm;
pub mod proc;
pub mod sched;
pub mod sync;
pub mod ipc;

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    logger::init();
    mm::init();
    hcf();
}

#[panic_handler]
fn rust_panic(info: &core::panic::PanicInfo) -> ! {
    log::error!("{}", info);
    hcf();
}

fn hcf() -> ! {
    arch::halt()
}
