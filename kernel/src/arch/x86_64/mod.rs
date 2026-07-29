//! x86_64 architecture support for PetraOS.

pub mod cpu;
pub mod power;
pub mod ptrace;
pub mod signal;

pub use cpu::*;
pub use power::*;
pub use ptrace::*;
pub use signal::*;

pub fn init() {
    power::init();
}
