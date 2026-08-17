//! ACPI table parser for discovering APIC hardware.
//!
//! Parses the RSDP → RSDT/XSDT → MADT chain to locate the Local APIC base
//! address, I/O APIC entries, and Interrupt Source Override entries needed
//! for interrupt controller initialization.
use crate::mm::ensure_mapped;

/// Standard ACPI System Description Table header (36 bytes).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct SdtHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

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
    const fn new() -> Self {
        Self {
            local_apic_address: 0,
            io_apics: [None; 8],
            io_apic_count: 0,
            isos: [None; 24],
            iso_count: 0,
        }
    }
}

/// Object representing the Root System Description Pointer (RSDP).
pub struct Rsdp {
    virt_addr: *const u8,
}

impl Rsdp {
    /// Create a new `Rsdp` instance from a virtual address.
    pub fn new(virt_addr: *const u8) -> Self {
        Self { virt_addr }
    }

    /// Read the ACPI revision.
    pub fn revision(&self) -> u8 {
        // SAFETY: The caller guarantees the RSDP is mapped and valid.
        unsafe { *self.virt_addr.add(15) }
    }

    /// Retrieve the physical address of the RSDT (32-bit).
    pub fn rsdt_physical_address(&self) -> u64 {
        // SAFETY: RSDT physical address is at offset 16 (4 bytes).
        (unsafe { core::ptr::read_unaligned(self.virt_addr.add(16) as *const u32) }) as u64
    }

    /// Retrieve the physical address of the XSDT (64-bit).
    pub fn xsdt_physical_address(&self) -> u64 {
        // SAFETY: XSDT physical address is at offset 24 (8 bytes).
        unsafe { core::ptr::read_unaligned(self.virt_addr.add(24) as *const u64) }
    }
}

/// Object representing an ACPI System Description Table (SDT).
pub struct Sdt {
    phys_addr: u64,
    virt_addr: *const u8,
}

impl Sdt {
    /// Locate, map, and instantiate an `Sdt` at the given physical address.
    pub fn new(phys_addr: u64) -> Self {
        // Map first 36 bytes (header size) to read the actual length
        ensure_mapped(phys_addr, core::mem::size_of::<SdtHeader>());

        let hhdm = crate::mm::hhdm_offset();
        let virt_addr = (phys_addr + hhdm) as *const u8;

        // Read header length and map the full table
        let header = unsafe { *(virt_addr as *const SdtHeader) };
        ensure_mapped(phys_addr, header.length as usize);

        Self {
            phys_addr,
            virt_addr,
        }
    }

    /// Access the standard SDT header.
    fn header(&self) -> SdtHeader {
        unsafe { *(self.virt_addr as *const SdtHeader) }
    }

    /// Get the table's signature.
    pub fn signature(&self) -> [u8; 4] {
        self.header().signature
    }

    /// Get the total table length.
    pub fn length(&self) -> usize {
        self.header().length as usize
    }

    /// Returns an iterator over table physical address pointers (RSDT/XSDT).
    ///
    /// If `use_64bit` is true, entries are parsed as 64-bit pointers; otherwise 32-bit pointers.
    pub fn table_pointers(&self, use_64bit: bool) -> TablePointers {
        let header_size = core::mem::size_of::<SdtHeader>();
        let data_len = self.length().saturating_sub(header_size);
        let ptr_size = if use_64bit { 8 } else { 4 };

        TablePointers {
            virt_base: unsafe { self.virt_addr.add(header_size) },
            count: data_len / ptr_size,
            use_64bit,
            index: 0,
        }
    }
}

/// Iterator over child table physical addresses listed in RSDT or XSDT.
pub struct TablePointers {
    virt_base: *const u8,
    count: usize,
    use_64bit: bool,
    index: usize,
}

impl Iterator for TablePointers {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }

        let addr = if self.use_64bit {
            unsafe { core::ptr::read_unaligned((self.virt_base as *const u64).add(self.index)) }
        } else {
            (unsafe { core::ptr::read_unaligned((self.virt_base as *const u32).add(self.index)) })
                as u64
        };

        self.index += 1;
        Some(addr)
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
        (unsafe { core::ptr::read_unaligned(self.sdt.virt_addr.add(36) as *const u32) }) as u64
    }

    /// Retrieve an iterator over the MADT's variable-length records/entries.
    pub fn entries(&self) -> MadtEntries {
        MadtEntries {
            virt_addr: self.sdt.virt_addr,
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
                    let id = unsafe { *self.virt_addr.add(current_offset + 2) };
                    let address = unsafe {
                        core::ptr::read_unaligned(
                            self.virt_addr.add(current_offset + 4) as *const u32
                        )
                    };
                    let gsi_base = unsafe {
                        core::ptr::read_unaligned(
                            self.virt_addr.add(current_offset + 8) as *const u32
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
                    let bus_source = unsafe { *self.virt_addr.add(current_offset + 2) };
                    let irq_source = unsafe { *self.virt_addr.add(current_offset + 3) };
                    let gsi = unsafe {
                        core::ptr::read_unaligned(
                            self.virt_addr.add(current_offset + 4) as *const u32
                        )
                    };
                    let flags = unsafe {
                        core::ptr::read_unaligned(
                            self.virt_addr.add(current_offset + 8) as *const u16
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
                    let address = unsafe {
                        core::ptr::read_unaligned(
                            self.virt_addr.add(current_offset + 4) as *const u64
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

/// Parse the ACPI tables starting from the RSDP provided by Limine.
///
/// Returns `MadtInfo` containing all discovered APIC hardware, or `None`
/// if the RSDP response is missing or the MADT cannot be found.
pub fn parse_madt() -> Option<MadtInfo> {
    let rsdp_response = crate::limine::RSDP_REQUEST.get_response()?;
    let rsdp_phys = rsdp_response.address() as *const u8 as u64;
    let hhdm = crate::mm::hhdm_offset();
    let rsdp_addr = (rsdp_phys + hhdm) as *const u8;
    log::info!(
        "RSDP Physical Address: {:#x}, Virtual: {:?}",
        rsdp_phys,
        rsdp_addr
    );

    // Map the RSDP
    ensure_mapped(rsdp_phys, 36);

    let rsdp = Rsdp::new(rsdp_addr);
    let use_64bit = rsdp.revision() >= 2;
    let parent_table_phys = if use_64bit {
        rsdp.xsdt_physical_address()
    } else {
        rsdp.rsdt_physical_address()
    };

    let parent_table = Sdt::new(parent_table_phys);

    // Iterate over child tables to find MADT ("APIC")
    for child_phys in parent_table.table_pointers(use_64bit) {
        let child_table = Sdt::new(child_phys);
        if &child_table.signature() == b"APIC" {
            let madt = Madt::new(child_table);
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

            return Some(info);
        }
    }

    None
}
