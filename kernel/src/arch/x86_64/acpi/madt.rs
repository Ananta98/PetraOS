//! ACPI Multiple APIC Description Table (MADT / "APIC") parser.
//!
//! Discovers the Local APIC base address, I/O APICs, and Interrupt Source Overrides (ISOs)
//! required for multi-core and interrupt routing setup.

use super::sdt::{find_table, Sdt};

/// Information about a single I/O APIC discovered in the MADT.
#[derive(Debug, Clone, Copy)]
pub struct IoApicEntry {
    pub id: u8,
    pub address: u32,
    pub gsi_base: u32,
}

/// An Interrupt Source Override entry from the MADT.
///
/// These entries describe remappings of ISA IRQs to different
/// Global System Interrupts (GSIs) with specific polarity and trigger modes.
#[derive(Debug, Clone, Copy)]
pub struct InterruptSourceOverride {
    pub bus_source: u8,
    pub irq_source: u8,
    pub gsi: u32,
    pub flags: u16,
}

/// Aggregated results from parsing the MADT.
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

/// Object representing the Multiple APIC Description Table (MADT).
pub struct Madt {
    sdt: Sdt,
}

impl Madt {
    /// Wrap a generic SDT as a MADT.
    pub fn new(sdt: Sdt) -> Self {
        Self { sdt }
    }

    /// Retrieve the default Local APIC base address (32-bit field at offset 36).
    pub fn local_apic_address(&self) -> u64 {
        if self.sdt.length() < 40 {
            return 0;
        }
        // SAFETY: Offset 36 (4 bytes) is guaranteed within table bounds when length >= 40.
        (unsafe { core::ptr::read_unaligned(self.sdt.virt_addr().add(36) as *const u32) }) as u64
    }

    /// Retrieve an iterator over the MADT's variable-length records/entries.
    pub fn entries(&self) -> MadtEntries {
        MadtEntries {
            virt_addr: self.sdt.virt_addr(),
            total_length: self.sdt.length(),
            offset: 44, // MADT entries start after header + LAPIC address (36 + 4) + flags (4)
        }
    }
}

/// Enum representing typed entries parsed from the MADT.
pub enum MadtEntry {
    IoApic(IoApicEntry),
    InterruptSourceOverride(InterruptSourceOverride),
    LocalApicAddressOverride(u64),
    Unsupported { entry_type: u8, length: usize },
}

/// Iterator yielding parsed entries of the MADT.
pub struct MadtEntries {
    virt_addr: *const u8,
    total_length: usize,
    offset: usize,
}

impl Iterator for MadtEntries {
    type Item = MadtEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset + 2 > self.total_length {
            return None;
        }

        // SAFETY: Bounds check above guarantees offset and offset + 1 are accessible.
        let entry_type = unsafe { *self.virt_addr.add(self.offset) };
        let entry_length = unsafe { *self.virt_addr.add(self.offset + 1) } as usize;

        if entry_length < 2 || self.offset + entry_length > self.total_length {
            return None;
        }

        let current_offset = self.offset;
        self.offset += entry_length;

        match entry_type {
            // Type 1: I/O APIC
            1 => {
                if entry_length >= 12 {
                    // SAFETY: `entry_length >= 12` ensures offset + 12 is within bounds.
                    let id = unsafe { *self.virt_addr.add(current_offset + 2) };
                    let address = unsafe {
                        core::ptr::read_unaligned(
                            self.virt_addr.add(current_offset + 4) as *const u32,
                        )
                    };
                    let gsi_base = unsafe {
                        core::ptr::read_unaligned(
                            self.virt_addr.add(current_offset + 8) as *const u32,
                        )
                    };
                    Some(MadtEntry::IoApic(IoApicEntry {
                        id,
                        address,
                        gsi_base,
                    }))
                } else {
                    Some(MadtEntry::Unsupported {
                        entry_type,
                        length: entry_length,
                    })
                }
            }
            // Type 2: Interrupt Source Override
            2 => {
                if entry_length >= 10 {
                    // SAFETY: `entry_length >= 10` ensures offset + 10 is within bounds.
                    let bus_source = unsafe { *self.virt_addr.add(current_offset + 2) };
                    let irq_source = unsafe { *self.virt_addr.add(current_offset + 3) };
                    let gsi = unsafe {
                        core::ptr::read_unaligned(
                            self.virt_addr.add(current_offset + 4) as *const u32,
                        )
                    };
                    let flags = unsafe {
                        core::ptr::read_unaligned(
                            self.virt_addr.add(current_offset + 8) as *const u16,
                        )
                    };
                    Some(MadtEntry::InterruptSourceOverride(
                        InterruptSourceOverride {
                            bus_source,
                            irq_source,
                            gsi,
                            flags,
                        },
                    ))
                } else {
                    Some(MadtEntry::Unsupported {
                        entry_type,
                        length: entry_length,
                    })
                }
            }
            // Type 5: Local APIC Address Override (64-bit)
            5 => {
                if entry_length >= 12 {
                    // SAFETY: `entry_length >= 12` ensures offset + 12 is within bounds.
                    let address = unsafe {
                        core::ptr::read_unaligned(
                            self.virt_addr.add(current_offset + 4) as *const u64,
                        )
                    };
                    Some(MadtEntry::LocalApicAddressOverride(address))
                } else {
                    Some(MadtEntry::Unsupported {
                        entry_type,
                        length: entry_length,
                    })
                }
            }
            _ => Some(MadtEntry::Unsupported {
                entry_type,
                length: entry_length,
            }),
        }
    }
}

/// Parse the ACPI tables starting from the RSDP to locate and parse the MADT.
///
/// Returns `MadtInfo` containing all discovered APIC hardware, or `None`
/// if the RSDP is missing or the MADT ("APIC") cannot be found.
pub fn parse_madt() -> Option<MadtInfo> {
    let madt_sdt = find_table(b"APIC")?;
    let madt = Madt::new(madt_sdt);
    let mut info = MadtInfo::new();
    info.local_apic_address = madt.local_apic_address();

    for entry in madt.entries() {
        match entry {
            MadtEntry::IoApic(io_apic) => {
                if info.io_apic_count < info.io_apics.len() {
                    info.io_apics[info.io_apic_count] = Some(io_apic);
                    info.io_apic_count += 1;
                }
            }
            MadtEntry::InterruptSourceOverride(iso) => {
                if info.iso_count < info.isos.len() {
                    info.isos[info.iso_count] = Some(iso);
                    info.iso_count += 1;
                }
            }
            MadtEntry::LocalApicAddressOverride(address) => {
                info.local_apic_address = address;
            }
            _ => {}
        }
    }

    Some(info)
}
