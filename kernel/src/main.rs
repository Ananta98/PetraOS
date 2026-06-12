#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

pub mod arch;
pub mod drivers;
pub mod fs;
pub mod limine;
pub mod logger;
pub mod mm;
pub mod proc;
pub mod sched;
pub mod sync;
pub mod syscalls;

use arch::{ArchImpl, CpuArch};

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    logger::init();
    mm::init();
    ArchImpl::init_hardware();
    drivers::pci::init().ok();
    drivers::framebuffer::init();
    fs::init();
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
