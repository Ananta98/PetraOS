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
mod modules;
mod net;
mod proc;
mod scheduler;
mod syscall;
mod vm;

#[ostd::main]
fn kernel_main() {
    arch::init();
    vm::init();
    irq::init();
    modules::init().expect("failed to initialize kernel modules");
    scheduler::init();
    fs::init().expect("failed to initialize filesystem");
    net::init();
    proc::spawn_init_process();

    loop {
        ostd::task::halt_cpu();
    }
}
