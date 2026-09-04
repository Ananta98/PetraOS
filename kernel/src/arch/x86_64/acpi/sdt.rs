//! ACPI System Description Table (SDT) header and table traversal.
//!
//! Provides the generic header representation (`SdtHeader`), the SDT table
//! wrapper (`Sdt`), child table pointers iterator (`TablePointers`), and
//! helper functions to search for tables by signature.

use super::rsdp::Rsdp;
use crate::mm::ensure_mapped;

/// Standard ACPI System Description Table header (36 bytes).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct SdtHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: u32,
    pub creator_revision: u32,
}

/// Object representing an ACPI System Description Table (SDT).
#[derive(Debug, Clone, Copy)]
pub struct Sdt {
    phys_addr: u64,
    virt_addr: *const u8,
}

impl Sdt {
    /// Locate, map, and instantiate an `Sdt` at the given physical address.
    pub fn new(phys_addr: u64) -> Self {
        // Map first 36 bytes (header size) to read the actual table length
        ensure_mapped(phys_addr, core::mem::size_of::<SdtHeader>());

        let hhdm = crate::mm::hhdm_offset();
        let virt_addr = (phys_addr + hhdm) as *const u8;

        // Read header length and map the full table
        // SAFETY: The header memory was mapped via `ensure_mapped` above.
        let header = unsafe { *(virt_addr as *const SdtHeader) };
        ensure_mapped(phys_addr, header.length as usize);

        Self {
            phys_addr,
            virt_addr,
        }
    }

    /// Access the physical address of this table.
    pub fn phys_addr(&self) -> u64 {
        self.phys_addr
    }

    /// Access the virtual address of this table.
    pub fn virt_addr(&self) -> *const u8 {
        self.virt_addr
    }

    /// Access the standard SDT header.
    pub fn header(&self) -> SdtHeader {
        // SAFETY: The table was mapped during instantiation in `new`.
        unsafe { *(self.virt_addr as *const SdtHeader) }
    }

    /// Get the table's 4-byte signature (e.g. `b"APIC"`, `b"HPET"`).
    pub fn signature(&self) -> [u8; 4] {
        self.header().signature
    }

    /// Get the total table length in bytes.
    pub fn length(&self) -> usize {
        self.header().length as usize
    }

    /// Returns an iterator over child table physical address pointers (from RSDT or XSDT).
    ///
    /// If `use_64bit` is true, entries are parsed as 64-bit pointers; otherwise 32-bit pointers.
    pub fn table_pointers(&self, use_64bit: bool) -> TablePointers {
        let header_size = core::mem::size_of::<SdtHeader>();
        let data_len = self.length().saturating_sub(header_size);
        let ptr_size = if use_64bit { 8 } else { 4 };

        TablePointers {
            // SAFETY: `header_size` is within the table length if data_len >= 0.
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

        // SAFETY: `self.index < self.count` ensures the read is within mapped table data.
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

/// Locate an ACPI table by its 4-byte signature (e.g., `b"APIC"`, `b"HPET"`).
///
/// Traverses the RSDP to the root RSDT/XSDT and searches all child table pointers.
pub fn find_table(signature: &[u8; 4]) -> Option<Sdt> {
    let rsdp = Rsdp::get_rsdp()?;
    let use_64bit = rsdp.revision() >= 2;
    let parent_table_phys = if use_64bit {
        rsdp.xsdt_physical_address()
    } else {
        rsdp.rsdt_physical_address()
    };

    if parent_table_phys == 0 {
        return None;
    }

    let parent_table = Sdt::new(parent_table_phys);
    for child_phys in parent_table.table_pointers(use_64bit) {
        if child_phys == 0 {
            continue;
        }
        let child_table = Sdt::new(child_phys);
        if &child_table.signature() == signature {
            return Some(child_table);
        }
    }

    None
}
