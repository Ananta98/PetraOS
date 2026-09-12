//! ACPI Subsystem for x86_64 Architecture.
//!
//! Powered by the `acpi` crate (v6.1.1). Provides discovery of system configuration,
//! APIC topology, and High Precision Event Timer (HPET).
//! Power management (poweroff / reboot) lives in [`power`].

pub mod handler;
pub mod power;

pub use handler::PetraAcpiHandler;

use acpi::sdt::hpet::HpetTable;
use acpi::sdt::madt::{Madt, MadtEntry};
use acpi::{AcpiError, AcpiTables, PhysicalMapping};

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
