//! ACPI Power Management for x86_64 Architecture.
//!
//! Consolidated power-state transitions:
//! - ACPI S5 Soft-Off (`acpi_poweroff`) and FADT reset (`acpi_reboot`)
//! - Full system `poweroff` / `reboot` with platform fallbacks
//!   (PCI reset, emulator ports, triple fault, CPU halt).

use super::{PetraAcpiHandler, get_tables};
use crate::arch::cpu::ports::Ports;
use crate::arch::disable_interrupts;
use crate::mm::ensure_mapped;
use acpi::PhysicalMapping;
use acpi::sdt::fadt::Fadt;
use core::arch::asm;

const PM1_CNT_SCI_EN: u16 = 1 << 0;
const PM1_CNT_SLP_EN: u16 = 1 << 13;
const PM1_CNT_SLP_TYP_SHIFT: u16 = 10;

/// Fast PCI reset control port.
const PCI_RESET_PORT: u16 = 0xCF9;
const PCI_RESET_PREPARE: u8 = 0x02;
const PCI_RESET_TRIGGER: u8 = 0x06;

/// Emulator/Hypervisor shutdown ports for fallback execution.
const QEMU_ACPI_SHUTDOWN_PORT: u16 = 0x604;
const QEMU_ACPI_SHUTDOWN_CMD: u16 = 0x2000;

const QEMU_ISA_DEBUG_EXIT_PORT: u16 = 0x501;
const QEMU_ISA_DEBUG_EXIT_CMD: u8 = 0x31;

/// Parse S5 sleep state values from DSDT AML bytecode.
fn parse_s5_from_dsdt(dsdt_phys: usize, length: usize) -> Option<(u16, u16)> {
    ensure_mapped(dsdt_phys as u64, length);
    let hhdm = crate::mm::hhdm_offset();
    let virt = (dsdt_phys as u64 + hhdm) as *const u8;
    // SAFETY: DSDT region was mapped via ensure_mapped and accessed via HHDM.
    let aml = unsafe { core::slice::from_raw_parts(virt, length) };

    let s5_pattern = b"_S5_";
    let mut offset = 36;
    while offset + 8 < length {
        if &aml[offset..offset + 4] == s5_pattern {
            let is_name_op = (offset > 0 && aml[offset - 1] == 0x08)
                || (offset > 1 && aml[offset - 2] == 0x08 && aml[offset - 1] == 0x5C);
            let pkg_offset = offset + 4;
            if is_name_op || aml[pkg_offset] == 0x12 {
                let pkg_start = if aml[pkg_offset] == 0x12 {
                    pkg_offset
                } else {
                    pkg_offset + 1
                };
                if pkg_start < length && aml[pkg_start] == 0x12 {
                    let pkg = &aml[pkg_start..];
                    if pkg.len() >= 5 {
                        let lead = pkg[1];
                        let extra = (lead >> 6) as usize;
                        let mut idx = 2 + extra + 1; // skip header + num_elements
                        let slp_a = read_aml_int(pkg, &mut idx)?;
                        let slp_b = read_aml_int(pkg, &mut idx).unwrap_or(slp_a);
                        return Some((slp_a, slp_b));
                    }
                }
            }
        }
        offset += 1;
    }
    None
}

fn read_aml_int(slice: &[u8], idx: &mut usize) -> Option<u16> {
    if *idx >= slice.len() {
        return None;
    }
    match slice[*idx] {
        0x00 => {
            *idx += 1;
            Some(0)
        }
        0x01 => {
            *idx += 1;
            Some(1)
        }
        0x0A => {
            if *idx + 1 >= slice.len() {
                return None;
            }
            let val = slice[*idx + 1] as u16;
            *idx += 2;
            Some(val)
        }
        0x0B => {
            if *idx + 2 >= slice.len() {
                return None;
            }
            let val = (slice[*idx + 1] as u16) | ((slice[*idx + 2] as u16) << 8);
            *idx += 3;
            Some(val)
        }
        val => {
            *idx += 1;
            Some(val as u16)
        }
    }
}

/// Perform an ACPI S5 Soft-Off (Shutdown).
pub fn acpi_poweroff() -> Result<(), &'static str> {
    let tables = get_tables().map_err(|_| "Failed to locate ACPI tables")?;
    let fadt_mapping: PhysicalMapping<PetraAcpiHandler, Fadt> =
        tables.find_table::<Fadt>().ok_or("FADT not found")?;
    let fadt = &*fadt_mapping;

    let dsdt = tables.dsdt().map_err(|_| "Failed to find DSDT")?;
    let (slp_typa, slp_typb) = parse_s5_from_dsdt(dsdt.phys_address, dsdt.length as usize)
        .ok_or("S5 sleep state not found in DSDT")?;

    let pm1a = fadt
        .pm1a_control_block()
        .map_err(|_| "Invalid PM1a control block")?;
    let pm1b = fadt.pm1b_control_block().unwrap_or(None);

    // Enable ACPI mode if not yet active
    if pm1a.address_space == acpi::address::AddressSpace::SystemIo {
        let pm1a_port = pm1a.address as u16;
        // SAFETY: PM1a control block is a standard ACPI SystemIo port from FADT.
        let pm1a_val = unsafe { Ports::inw(pm1a_port) };
        if pm1a_val & PM1_CNT_SCI_EN == 0 && fadt.smi_cmd_port != 0 && fadt.acpi_enable != 0 {
            // SAFETY: SMI command port + ACPI enable value come from FADT.
            unsafe {
                Ports::outb(fadt.smi_cmd_port as u16, fadt.acpi_enable);
            }
            for _ in 0..10_000 {
                // SAFETY: Polling PM1a SystemIo port from FADT.
                if unsafe { Ports::inw(pm1a_port) } & PM1_CNT_SCI_EN != 0 {
                    break;
                }
                crate::arch::cpu::control::halt();
            }
        }
    }

    let val_a = (slp_typa << PM1_CNT_SLP_TYP_SHIFT) | PM1_CNT_SLP_EN;
    let val_b = (slp_typb << PM1_CNT_SLP_TYP_SHIFT) | PM1_CNT_SLP_EN;

    if pm1a.address_space == acpi::address::AddressSpace::SystemIo {
        // SAFETY: Writing S5 sleep values to PM1a SystemIo port from FADT.
        unsafe {
            Ports::outw(pm1a.address as u16, val_a);
        }
    }
    if let Some(pm1b_gas) = pm1b {
        if pm1b_gas.address_space == acpi::address::AddressSpace::SystemIo {
            // SAFETY: Writing S5 sleep values to PM1b SystemIo port from FADT.
            unsafe {
                Ports::outw(pm1b_gas.address as u16, val_b);
            }
        }
    }

    Ok(())
}

/// Perform an ACPI hardware reboot via the FADT `reset_reg`.
pub fn acpi_reboot() -> Result<(), &'static str> {
    let tables = get_tables().map_err(|_| "Failed to locate ACPI tables")?;
    let fadt_mapping: PhysicalMapping<PetraAcpiHandler, Fadt> =
        tables.find_table::<Fadt>().ok_or("FADT not found")?;
    let fadt = &*fadt_mapping;

    let flags = fadt.flags;
    if !flags.supports_system_reset_via_fadt() {
        return Err("FADT does not support reset register");
    }

    let reset_value = fadt.reset_value;
    let reset_reg = fadt
        .reset_register()
        .map_err(|_| "Invalid reset register GAS")?;
    match reset_reg.address_space {
        acpi::address::AddressSpace::SystemIo => {
            // SAFETY: Reset register SystemIo port comes from FADT.
            unsafe {
                Ports::outb(reset_reg.address as u16, reset_value);
            }
            Ok(())
        }
        acpi::address::AddressSpace::SystemMemory => {
            ensure_mapped(reset_reg.address, 1);
            let hhdm = crate::mm::hhdm_offset();
            let virt = (reset_reg.address + hhdm) as *mut u8;
            // SAFETY: Reset register memory was mapped via ensure_mapped.
            unsafe {
                core::ptr::write_volatile(virt, reset_value);
            }
            Ok(())
        }
        _ => Err("Unsupported reset register address space"),
    }
}

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

/// Perform a full system reboot with cascading fallbacks.
///
/// 1. ACPI FADT hardware reset register
/// 2. Fast PCI reset (port 0xCF9)
/// 3. Triple fault (forced CPU reset via invalid IDT)
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

/// Perform a full system shutdown / poweroff.
///
/// Uses the standard ACPI S5 sleep state with inline virtual machine /
/// emulator fallback mechanisms (QEMU / Bochs), terminating with an idle
/// CPU halt loop if poweroff fails.
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
