//! ACPI table parsing and hardware discovery subsystem for x86_64.
//!
//! Submodules provide support for:
//! - [`rsdp`]: Root System Description Pointer parsing.
//! - [`sdt`]: System Description Table headers, traversal, and lookup.
//! - [`madt`]: Multiple APIC Description Table parsing (LAPIC, IOAPIC, ISOs).

pub mod madt;
pub mod rsdp;
pub mod sdt;

pub use madt::{
    InterruptSourceOverride, MadtInfo, parse_madt,
};
pub use sdt::find_table;
