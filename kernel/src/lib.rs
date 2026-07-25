#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

#[cfg_attr(target_arch = "x86_64", path = "arch/x86_64/mod.rs")]
mod arch;

mod device;
mod drivers;
mod fs;
mod ipc;
mod irq;
mod net;
mod proc;
mod scheduler;
mod syscall;
mod vm;

#[ostd::main]
fn kernel_main() {
    arch::power::init();
    vm::init();
    irq::init();
    device::manager::init();
    net::init();
    fs::init().expect("failed to initialize filesystem");
    proc::spawn_init_process();
    scheduler::init();
}
