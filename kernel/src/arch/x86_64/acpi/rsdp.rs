//! ACPI Root System Description Pointer (RSDP) parser.
//!
//! Provides structures and helpers to locate and parse the ACPI RSDP,
//! determining whether the system uses ACPI 1.0 (RSDT) or ACPI 2.0+ (XSDT).

use crate::mm::ensure_mapped;

/// Object representing the Root System Description Pointer (RSDP).
#[derive(Debug, Clone, Copy)]
pub struct Rsdp {
    virt_addr: *const u8,
}

impl Rsdp {
    /// Create a new `Rsdp` instance from a virtual address.
    pub const fn new(virt_addr: *const u8) -> Self {
        Self { virt_addr }
    }

    /// Retrieve the RSDP from the Limine bootloader response, mapping it into virtual memory.
    pub fn get_rsdp() -> Option<Self> {
        let rsdp_response = crate::limine::RSDP_REQUEST.get_response()?;
        let rsdp_phys = rsdp_response.address() as *const u8 as u64;
        let hhdm = crate::mm::hhdm_offset();
        let rsdp_addr = (rsdp_phys + hhdm) as *const u8;

        // Ensure first 36 bytes (ACPI 2.0 RSDP length) are mapped
        ensure_mapped(rsdp_phys, 36);

        Some(Self::new(rsdp_addr))
    }

    /// Read the ACPI revision (byte offset 15).
    ///
    /// 0 indicates ACPI 1.0 (uses RSDT).
    /// 2 or higher indicates ACPI 2.0+ (uses XSDT).
    pub fn revision(&self) -> u8 {
        // SAFETY: The caller guarantees or `get_rsdp` ensures the RSDP is mapped and valid.
        unsafe { *self.virt_addr.add(15) }
    }

    /// Retrieve the physical address of the RSDT (32-bit pointer at byte offset 16).
    pub fn rsdt_physical_address(&self) -> u64 {
        // SAFETY: The RSDT physical address is a 32-bit integer at offset 16, within mapped bounds.
        (unsafe { core::ptr::read_unaligned(self.virt_addr.add(16) as *const u32) }) as u64
    }

    /// Retrieve the physical address of the XSDT (64-bit pointer at byte offset 24).
    pub fn xsdt_physical_address(&self) -> u64 {
        // SAFETY: The XSDT physical address is a 64-bit integer at offset 24, within mapped bounds.
        unsafe { core::ptr::read_unaligned(self.virt_addr.add(24) as *const u64) }
    }
}
