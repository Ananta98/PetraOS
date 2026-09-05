//! Architecture-specific implementation for x86_64.
//!
//! This module acts as the public interface for the x86_64 platform layer,
//! exposing hardware initialization, CPU control, interrupt management,
//! paging, scheduling, and device drivers through unified re-exports.

pub mod acpi;
pub mod cpu;
pub mod interrupt;
pub mod paging;
pub mod sched;
pub mod signal;
pub mod syscall;
pub mod timer;

use core::sync::atomic::{AtomicBool, Ordering};
pub use cpu::control::{cpu_count, cpu_id, enable_and_hlt, idle};
pub use cpu::gdt;
pub use cpu::ports;
pub use cpu::tss;
pub use cpu::userspace;
pub use cpu::{active_address_space_root, read_cr2, set_address_space_root};
pub use interrupt::flags::{disable_interrupts, enable_interrupts, without_interrupts};
pub use interrupt::idt;
pub use interrupt::interrupts;
pub use interrupt::lapic;
pub use sched::arch_switch_context;
pub use timer::lapic_timer;

static ARCH_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Main architecture hardware initialization entry point.
///
/// Orchestrates CPU (GDT, SSE, SYSCALL MSRs), ACPI MADT parsing,
/// interrupt controllers (IDT, PIC, LAPIC, IOAPIC), hardware timers,
/// and boots Application Processors (APs) via SMP.
pub fn init() -> Result<(), &'static str> {
    if ARCH_INITIALIZED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    cpu::init();

    let madt_info = acpi::parse_madt()
        .ok_or("Failed to parse ACPI MADT — APIC initialization requires MADT")?;

    log::info!(
        "MADT parsed: LAPIC base={:#x}, {} IOAPIC(s), {} ISO(s).",
        madt_info.local_apic_address,
        madt_info.io_apic_count,
        madt_info.iso_count
    );

    interrupt::init(&madt_info);
    timer::init();

    interrupt::enable_interrupts();

    // Start Application Processors now that the BSP is fully online.
    cpu::smp::start_aps();

    Ok(())
}

crate::arch_initcall!(init);
