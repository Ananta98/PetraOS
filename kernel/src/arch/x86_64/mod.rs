//! Architecture-specific implementation for x86_64.
//!
//! This module acts as the public interface for the x86_64 platform layer,
//! exposing hardware initialization, CPU control, interrupt management,
//! paging, scheduling, and device drivers through unified re-exports.

#![allow(dead_code)]

pub mod acpi;
pub mod cpu;
pub mod init;
pub mod interrupt;
pub mod paging;
pub mod sched;
pub mod signal;
pub mod syscall;
pub mod timer;

pub use cpu::control::{cpu_count, cpu_id, enable_and_hlt, idle};
pub use cpu::gdt;
pub use cpu::ports;
pub use cpu::tss;
pub use cpu::userspace;
pub use cpu::{active_address_space_root, read_cr2, set_address_space_root};
pub use init::init;
pub use interrupt::flags::{disable_interrupts, enable_interrupts, without_interrupts};
pub use interrupt::idt;
pub use interrupt::interrupts;
pub use interrupt::lapic;
pub use sched::arch_switch_context;
pub use timer::lapic_timer;
