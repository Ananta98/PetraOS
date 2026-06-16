#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

pub mod vm;

#[ostd::main]
fn kernel_main() {
    vm::init();
}
