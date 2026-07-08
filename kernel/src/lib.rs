#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

mod proc;
mod vm;

#[ostd::main]
fn kernel_main() {
    vm::init();
    proc::init();
}
