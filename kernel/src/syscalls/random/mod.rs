//! Syscall handlers for random number generation.

pub mod random;

pub use random::sys_getrandom;
