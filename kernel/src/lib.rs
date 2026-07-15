#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

mod drivers;
mod fs;
mod ipc;
mod proc;
mod scheduler;
mod syscall;
mod vm;

#[ostd::main]
fn kernel_main() {
    vm::init();
    drivers::init();
    fs::init().expect("failed to initialize filesystem");
    proc::spawn_init_process();
    scheduler::init();
}
