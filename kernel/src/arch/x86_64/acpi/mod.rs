//! ACPI Subsystem for x86_64 Architecture.
//!
//! Powered by the `acpi` crate (v6.1.1). Provides discovery of system configuration,
//! APIC topology, High Precision Event Timer (HPET), and ACPI poweroff / reboot operations.

pub mod handler;

pub use handler::PetraAcpiHandler;

use acpi::sdt::fadt::Fadt;
use acpi::sdt::hpet::HpetTable;
use acpi::sdt::madt::{Madt, MadtEntry};
use acpi::{AcpiError, AcpiTables, PhysicalMapping};
use crate::arch::cpu::ports::Ports;
use crate::mm::ensure_mapped;

// ── Compatibility Types for Kernel Drivers ────────────────────────────────────

/// Information about a single I/O APIC discovered in the MADT.
#[derive(Debug, Clone, Copy)]
pub struct IoApicEntry {
    pub id: u8,
    pub address: u32,
    pub gsi_base: u32,
}

/// An Interrupt Source Override entry from the MADT.
#[derive(Debug, Clone, Copy)]
pub struct InterruptSourceOverride {
    pub bus_source: u8,
    pub irq_source: u8,
    pub gsi: u32,
    pub flags: u16,
}

/// Aggregated results from parsing the MADT.
#[derive(Debug, Clone, Copy)]
pub struct MadtInfo {
    pub local_apic_address: u64,
    pub io_apics: [Option<IoApicEntry>; 8],
    pub io_apic_count: usize,
    pub isos: [Option<InterruptSourceOverride>; 24],
    pub iso_count: usize,
}

impl MadtInfo {
    pub const fn new() -> Self {
        Self {
            local_apic_address: 0,
            io_apics: [None; 8],
            io_apic_count: 0,
            isos: [None; 24],
            iso_count: 0,
        }
    }
}

// ── ACPI Table Access & Hardware Discovery ────────────────────────────────────

/// Construct an `AcpiTables` instance using the Limine bootloader RSDP pointer.
pub fn get_tables() -> Result<AcpiTables<PetraAcpiHandler>, AcpiError> {
    let rsdp_response = crate::limine::RSDP_REQUEST
        .get_response()
        .ok_or(AcpiError::RsdpIncorrectSignature)?;
    let rsdp_phys = rsdp_response.address() as *const u8 as usize;

    let handler = PetraAcpiHandler::new();
    // SAFETY: Bootloader guarantees valid RSDP physical address.
    unsafe { AcpiTables::from_rsdp(handler, rsdp_phys) }
}

/// Parse the MADT ("APIC") table and extract LAPIC address, I/O APICs, and ISOs.
pub fn parse_madt() -> Option<MadtInfo> {
    let tables = get_tables().ok()?;
    let madt_mapping: PhysicalMapping<PetraAcpiHandler, Madt> = tables.find_table::<Madt>()?;
    let madt = madt_mapping.get();

    let mut info = MadtInfo::new();
    info.local_apic_address = madt.local_apic_address as u64;

    for entry in madt.entries() {
        match entry {
            MadtEntry::LocalApicAddressOverride(override_entry) => {
                info.local_apic_address = override_entry.local_apic_address;
            }
            MadtEntry::IoApic(io_apic) => {
                if info.io_apic_count < info.io_apics.len() {
                    info.io_apics[info.io_apic_count] = Some(IoApicEntry {
                        id: io_apic.io_apic_id,
                        address: io_apic.io_apic_address,
                        gsi_base: io_apic.global_system_interrupt_base,
                    });
                    info.io_apic_count += 1;
                }
            }
            MadtEntry::InterruptSourceOverride(iso) => {
                if info.iso_count < info.isos.len() {
                    info.isos[info.iso_count] = Some(InterruptSourceOverride {
                        bus_source: iso.bus,
                        irq_source: iso.irq,
                        gsi: iso.global_system_interrupt,
                        flags: iso.flags,
                    });
                    info.iso_count += 1;
                }
            }
            _ => {}
        }
    }

    Some(info)
}

/// Locate and parse the HPET base physical address from the HPET ACPI table.
pub fn parse_hpet_base() -> Option<u64> {
    let tables = get_tables().ok()?;
    let hpet_mapping: PhysicalMapping<PetraAcpiHandler, HpetTable> =
        tables.find_table::<HpetTable>()?;
    let base = hpet_mapping.base_address.address;
    if base != 0 {
        Some(base)
    } else {
        None
    }
}

// ── ACPI Power Management (Poweroff & Reset) ──────────────────────────────────

const PM1_CNT_SCI_EN: u16 = 1 << 0;
const PM1_CNT_SLP_EN: u16 = 1 << 13;
const PM1_CNT_SLP_TYP_SHIFT: u16 = 10;

/// Parse S5 sleep state values from DSDT AML bytecode.
fn parse_s5_from_dsdt(dsdt_phys: usize, length: usize) -> Option<(u16, u16)> {
    ensure_mapped(dsdt_phys as u64, length);
    let hhdm = crate::mm::hhdm_offset();
    let virt = (dsdt_phys as u64 + hhdm) as *const u8;
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

    let pm1a = fadt.pm1a_control_block().map_err(|_| "Invalid PM1a control block")?;
    let pm1b = fadt.pm1b_control_block().unwrap_or(None);

    // Enable ACPI mode if not yet active
    if pm1a.address_space == acpi::address::AddressSpace::SystemIo {
        let pm1a_port = pm1a.address as u16;
        let pm1a_val = unsafe { Ports::inw(pm1a_port) };
        if pm1a_val & PM1_CNT_SCI_EN == 0 && fadt.smi_cmd_port != 0 && fadt.acpi_enable != 0 {
            unsafe {
                Ports::outb(fadt.smi_cmd_port as u16, fadt.acpi_enable);
            }
            for _ in 0..10_000 {
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
        unsafe {
            Ports::outw(pm1a.address as u16, val_a);
        }
    }
    if let Some(pm1b_gas) = pm1b {
        if pm1b_gas.address_space == acpi::address::AddressSpace::SystemIo {
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
    let reset_reg = fadt.reset_register().map_err(|_| "Invalid reset register GAS")?;
    match reset_reg.address_space {
        acpi::address::AddressSpace::SystemIo => {
            unsafe {
                Ports::outb(reset_reg.address as u16, reset_value);
            }
            Ok(())
        }
        acpi::address::AddressSpace::SystemMemory => {
            ensure_mapped(reset_reg.address, 1);
            let hhdm = crate::mm::hhdm_offset();
            let virt = (reset_reg.address + hhdm) as *mut u8;
            unsafe {
                core::ptr::write_volatile(virt, reset_value);
            }
            Ok(())
        }
        _ => Err("Unsupported reset register address space"),
    }
}
