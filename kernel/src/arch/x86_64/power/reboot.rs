//! Hardware and Platform System Reset Mechanisms.
//!
//! Provides system reboot operations with cascading fallbacks:
//! 1. ACPI FADT hardware reset register
//! 2. Fast PCI reset (port 0xCF9)
//! 3. Triple fault (forced CPU reset via invalid IDT)

use crate::arch::acpi::acpi_reboot;
use crate::arch::cpu::ports::Ports;
use crate::arch::disable_interrupts;
use core::arch::asm;

/// Fast PCI reset control port.
const PCI_RESET_PORT: u16 = 0xCF9;
const PCI_RESET_PREPARE: u8 = 0x02;
const PCI_RESET_TRIGGER: u8 = 0x06;

/// Attempt to reboot via Fast PCI reset (port 0xCF9).
fn pci_reset() {
    // SAFETY: Writing standard PCI reset control sequence.
    unsafe {
        Ports::outb(PCI_RESET_PORT, PCI_RESET_PREPARE);
        Ports::outb(PCI_RESET_PORT, PCI_RESET_TRIGGER);
    }
}

/// Force a CPU reset by deliberately triggering a triple fault.
///
/// Loads an empty IDT with limit 0 and triggers an interrupt, forcing the CPU
/// to generate a Double Fault followed immediately by a Triple Fault, which resets
/// the processor hardware.
fn emergency_cpu_reset() -> ! {
    #[repr(C, packed)]
    struct Idtr {
        limit: u16,
        base: u64,
    }

    let idtr = Idtr { limit: 0, base: 0 };

    // SAFETY: Disabling interrupts, loading zeroed IDT descriptor, and executing software int.
    unsafe {
        asm!(
            "lidt [{}]",
            "int 3",
            in(reg) &idtr,
            options(noreturn)
        );
    }
}

/// Perform a full system reboot with cascading fallbacks.
pub fn reboot() -> ! {
    disable_interrupts();
    log::info!("System reboot requested — initiating hardware reset sequence...");

    // Strategy 1: ACPI Reset Register
    if let Err(e) = acpi_reboot() {
        log::debug!("ACPI reboot unavailable: {}", e);
    }

    // Small delay to allow ACPI reset to latch
    for _ in 0..100_000 {
        core::hint::spin_loop();
    }

    // Strategy 2: Fast PCI Reset
    log::debug!("Attempting Fast PCI reset (0xCF9)...");
    pci_reset();

    for _ in 0..100_000 {
        core::hint::spin_loop();
    }

    // Strategy 3: Triple Fault
    log::warn!("Reset strategies exhausted, forcing triple fault...");
    emergency_cpu_reset()
}
