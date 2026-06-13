#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use ostd::prelude::*;

#[ostd::main]
fn kernel_main() {
    println!("Hello World");
}
