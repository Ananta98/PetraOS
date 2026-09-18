//! PCI MSI-X (Extended Message Signaled Interrupts) Capability
//!
//! Provides discovery, MMIO mapping, and vector configuration for PCI devices
//! implementing MSI-X according to the PCI Local Bus Specification.
//!
//! @author Ananta <kusumaananta042@gmail.com>

use crate::device::DriverError;
use crate::drivers::pci::config;
use crate::drivers::pci::device::PciDevice;
use crate::mm::{hhdm_offset, map_mmio};

/// PCI Capability ID for MSI-X.
pub const PCI_CAP_ID_MSIX: u8 = 0x11;

// MSI-X Configuration Space Register Offsets
const MSIX_MSG_CTRL_OFFSET: u8 = 0x02;
const MSIX_TABLE_OFFSET_BIR: u8 = 0x04;
const MSIX_PBA_OFFSET_BIR: u8 = 0x08;

// MSI-X Control bitmasks
const MSIX_CTRL_ENABLE: u16 = 1 << 15;
const MSIX_CTRL_TABLE_SIZE_MASK: u16 = 0x07FF;

// BIR and Offset masks
const MSIX_BIR_MASK: u32 = 0x07;
const MSIX_OFFSET_MASK: u32 = !0x07;

const MSIX_ENTRY_SIZE: usize = 16;
const MSIX_VECTOR_CTRL_MASKED: u32 = 1 << 0;

/// Specific error conditions encountered during MSI-X operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsixError {
    NotSupported,
    NotInitialized,
    IndexOutOfBounds,
    InvalidBar,
    MappingFailed,
}

impl From<MsixError> for DriverError {
    fn from(err: MsixError) -> Self {
        match err {
            MsixError::NotSupported => DriverError::Unsupported,
            MsixError::NotInitialized => DriverError::InitFailed,
            MsixError::IndexOutOfBounds => DriverError::InvalidBlock,
            MsixError::InvalidBar => DriverError::NoDevice,
            MsixError::MappingFailed => DriverError::AllocFailed,
        }
    }
}

/// Hardware controller and runtime state for PCI MSI-X.
#[derive(Debug)]
pub struct MsixCapability {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub cap_offset: u8,
    pub table_size: u16,
    pub table_bir: u8,
    pub table_offset: u32,
    pub table_phys: u64,
    pub table_virt: *mut u8,
    pub pba_bir: u8,
    pub pba_offset: u32,
    pub pba_phys: u64,
    pub pba_virt: *mut u8,
    pub initialized: bool,
}

// SAFETY: MSI-X structures manage memory-mapped device registers safely through volatile operations.
unsafe impl Send for MsixCapability {}
unsafe impl Sync for MsixCapability {}

impl MsixCapability {
    /// Probe for MSI-X capability on the specified PCI device without mapping MMIO.
    pub fn probe(pci_dev: &PciDevice) -> Option<Self> {
        let cap_offset = pci_dev.find_capability(PCI_CAP_ID_MSIX)?;
        let msg_ctrl = pci_dev.read_u16(cap_offset + MSIX_MSG_CTRL_OFFSET);
        let table_size = (msg_ctrl & MSIX_CTRL_TABLE_SIZE_MASK) + 1;

        let table_dword = pci_dev.read_u32(cap_offset + MSIX_TABLE_OFFSET_BIR);
        let table_bir = (table_dword & MSIX_BIR_MASK) as u8;
        let table_offset = table_dword & MSIX_OFFSET_MASK;

        let pba_dword = pci_dev.read_u32(cap_offset + MSIX_PBA_OFFSET_BIR);
        let pba_bir = (pba_dword & MSIX_BIR_MASK) as u8;
        let pba_offset = pba_dword & MSIX_OFFSET_MASK;

        let table_bar_phys = pci_dev.bar_phys(table_bir)?;
        let table_phys = table_bar_phys + table_offset as u64;

        let pba_bar_phys = pci_dev.bar_phys(pba_bir)?;
        let pba_phys = pba_bar_phys + pba_offset as u64;

        Some(Self {
            bus: pci_dev.bus,
            device: pci_dev.device,
            function: pci_dev.function,
            cap_offset,
            table_size,
            table_bir,
            table_offset,
            table_phys,
            table_virt: core::ptr::null_mut(),
            pba_bir,
            pba_offset,
            pba_phys,
            pba_virt: core::ptr::null_mut(),
            initialized: false,
        })
    }

    /// Initialize and map the MSI-X Table and PBA MMIO regions into kernel virtual space.
    pub fn init(&mut self) -> Result<(), MsixError> {
        if self.initialized {
            return Ok(());
        }

        let table_bytes = self.table_size as usize * MSIX_ENTRY_SIZE;
        if table_bytes == 0 {
            return Err(MsixError::MappingFailed);
        }

        // Map Table MMIO region
        map_mmio(self.table_phys, table_bytes);
        let hhdm = hhdm_offset();
        self.table_virt = (self.table_phys + hhdm) as *mut u8;

        if self.table_virt.is_null() {
            return Err(MsixError::MappingFailed);
        }

        // Map PBA MMIO region (8 bytes per 64 vectors)
        let pba_bytes = ((self.table_size as usize + 63) / 64) * 8;
        if pba_bytes > 0 {
            map_mmio(self.pba_phys, pba_bytes);
            self.pba_virt = (self.pba_phys + hhdm) as *mut u8;
        }

        self.initialized = true;

        // Mask all vector entries in the table initially as mandated by PCI spec
        for i in 0..self.table_size {
            let offset = i as usize * MSIX_ENTRY_SIZE;
            // SAFETY: Bounds check confirmed and table_virt points to mapped MMIO.
            unsafe {
                let ctrl_ptr = self.table_virt.add(offset + 12) as *mut u32;
                let val = core::ptr::read_volatile(ctrl_ptr);
                core::ptr::write_volatile(ctrl_ptr, val | MSIX_VECTOR_CTRL_MASKED);
            }
        }

        Ok(())
    }

    /// Enable the MSI-X capability for this PCI function.
    pub fn enable(&mut self) {
        let ctrl = config::read_u16(
            self.bus,
            self.device,
            self.function,
            self.cap_offset + MSIX_MSG_CTRL_OFFSET,
        );
        config::write_u16(
            self.bus,
            self.device,
            self.function,
            self.cap_offset + MSIX_MSG_CTRL_OFFSET,
            ctrl | MSIX_CTRL_ENABLE,
        );
    }

    /// Number of interrupt vectors supported in this device's MSI-X table.
    pub fn table_size(&self) -> u16 {
        self.table_size
    }

    /// Configure a specific MSI-X table entry targeting a CPU APIC ID and vector.
    pub fn configure_vector(
        &mut self,
        index: u16,
        apic_id: u8,
        vector: u8,
        masked: bool,
    ) -> Result<(), MsixError> {
        if !self.initialized || self.table_virt.is_null() {
            return Err(MsixError::NotInitialized);
        }
        if index >= self.table_size {
            return Err(MsixError::IndexOutOfBounds);
        }

        let offset = index as usize * MSIX_ENTRY_SIZE;
        let addr_lo = config::msi_address(apic_id);
        let addr_hi = 0u32;
        let data = config::msi_data(vector) as u32;
        let ctrl = if masked { MSIX_VECTOR_CTRL_MASKED } else { 0 };

        // SAFETY: Bounds check confirmed index < table_size and table_virt points to mapped MMIO.
        // Writes are 32-bit aligned and performed volatile as required by PCI Local Bus Specification.
        unsafe {
            let entry_ptr = self.table_virt.add(offset) as *mut u32;
            core::ptr::write_volatile(entry_ptr.add(3), MSIX_VECTOR_CTRL_MASKED);
            core::ptr::write_volatile(entry_ptr.add(0), addr_lo);
            core::ptr::write_volatile(entry_ptr.add(1), addr_hi);
            core::ptr::write_volatile(entry_ptr.add(2), data);
            core::ptr::write_volatile(entry_ptr.add(3), ctrl);
        }

        Ok(())
    }
}

/// Helper function to probe and initialize MSI-X for a given PCI device in a single step.
pub fn init_device(pci_dev: &PciDevice) -> Result<MsixCapability, DriverError> {
    let mut msix = MsixCapability::probe(pci_dev).ok_or(DriverError::NoDevice)?;
    msix.init()?;
    Ok(msix)
}
