//! Architecture Hardware Initialization Sequence.
//!
//! Orchestrates the initialization of CPU (GDT, SSE, SYSCALL MSRs), ACPI tables,
//! interrupt controllers (IDT, PIC, LAPIC, IOAPIC), hardware timers (HPET, LAPIC timer),
//! and boots Application Processors (APs) via SMP.

use crate::arch::acpi;
use crate::arch::cpu;
use crate::arch::interrupt;
use crate::arch::interrupt::enable_interrupts;
use crate::arch::timer;

/// Main architecture hardware initialization entry point.
pub fn init() {
    cpu::init();

    let madt_info =
        acpi::parse_madt().expect("Failed to parse ACPI MADT — APIC initialization requires MADT");

    log::info!(
        "MADT parsed: LAPIC base={:#x}, {} IOAPIC(s), {} ISO(s).",
        madt_info.local_apic_address,
        madt_info.io_apic_count,
        madt_info.iso_count
    );

    interrupt::init(&madt_info);
    timer::init();

    enable_interrupts();

    // Start Application Processors now that the BSP is fully online.
    cpu::smp::start_aps();
}
