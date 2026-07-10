#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

mod drivers;
mod fs;
mod proc;
mod vm;

#[ostd::main]
fn kernel_main() {
    vm::init();
    drivers::init();
    proc::init();
}
