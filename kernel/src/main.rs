#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

pub mod arch;
pub mod drivers;
pub mod limine;
pub mod logger;
pub mod mm;
pub mod proc;
pub mod sched;
pub mod sync;
pub mod syscalls;

use arch::{ArchImpl, CpuArch};
use core::arch::asm;

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    logger::init();
    mm::init();
    ArchImpl::init_hardware();

    drivers::framebuffer::init();

    log::info!("PetraOS Kernel Scaffolding Initialized.");
    log::warn!("Testing warning log levels!");
    log::error!("Testing error log levels!");

    hcf();
}

#[panic_handler]
fn rust_panic(info: &core::panic::PanicInfo) -> ! {
    log::error!("{}", info);
    hcf();
}

fn hcf() -> ! {
    loop {
       <ArchImpl as CpuArch>::halt();
    }
}
