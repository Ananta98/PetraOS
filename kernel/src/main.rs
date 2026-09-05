#![no_std]
#![no_main]
#![allow(dead_code)]

extern crate alloc;

#[cfg_attr(target_arch = "x86_64", path = "arch/x86_64/mod.rs")]
mod arch;

pub mod device;
pub mod drivers;
pub mod fs;
pub mod ipc;
pub mod limine;
pub mod logger;
pub mod mm;
pub mod modules;
pub mod net;
pub mod panic;
pub mod proc;
pub mod sched;
pub mod sync;
pub mod syscalls;
pub mod utils;

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    logger::init();
    mm::init();
    modules::init();
    sched::init();
    proc::process::init_proc::run_init_process();
}
