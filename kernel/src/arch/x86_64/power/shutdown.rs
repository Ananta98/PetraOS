//! System Shutdown and Poweroff Mechanisms.
//!
//! Provides system poweroff operations with standard ACPI S5 sleep state
//! and inline virtual machine / emulator fallback mechanisms (QEMU / Bochs),
//! terminating with an idle CPU halt loop if poweroff fails.

use crate::arch::acpi::acpi_poweroff;
use crate::arch::cpu::ports::Ports;
use crate::arch::disable_interrupts;

/// Emulator/Hypervisor shutdown ports for fallback execution.
const QEMU_ACPI_SHUTDOWN_PORT: u16 = 0x604;
const QEMU_ACPI_SHUTDOWN_CMD: u16 = 0x2000;

const QEMU_ISA_DEBUG_EXIT_PORT: u16 = 0x501;
const QEMU_ISA_DEBUG_EXIT_CMD: u8 = 0x31;

/// Attempt hypervisor/emulator shutdown via standard I/O ports.
fn fallback_emulator_poweroff() {
    // SAFETY: Writing standard emulator shutdown commands to well-known hypervisor ports.
    unsafe {
        // QEMU ACPI shutdown port (standard for modern QEMU i440fx/q35)
        Ports::outw(QEMU_ACPI_SHUTDOWN_PORT, QEMU_ACPI_SHUTDOWN_CMD);

        // QEMU isa-debug-exit device port
        Ports::outb(QEMU_ISA_DEBUG_EXIT_PORT, QEMU_ISA_DEBUG_EXIT_CMD);
    }
}

/// Perform a full system shutdown / poweroff.
pub fn poweroff() -> ! {
    disable_interrupts();
    log::info!("System poweroff requested — powering down...");

    // Strategy 1: Standard ACPI S5 Soft-Off
    if let Err(e) = acpi_poweroff() {
        log::warn!("ACPI S5 shutdown failed: {}", e);
    }

    // Delay to give the chipset / power plane time to latch S5 transition
    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    // Strategy 2: Virtualized environment fallback
    log::debug!("Attempting hypervisor/emulator fallback shutdown...");
    fallback_emulator_poweroff();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    // If hardware failed to cut power, halt the processor safely.
    log::warn!("Poweroff commands completed but machine remains powered. Halting CPU...");
    crate::arch::cpu::control::idle()
}
