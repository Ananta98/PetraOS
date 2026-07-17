//! MSI/MSI-X interrupt support for PCI devices.
//!
//! Message Signalled Interrupts (MSI) and MSI-X allow PCIe devices to
//! generate interrupts by writing a small data packet to a memory-mapped
//! address, instead of asserting a dedicated IRQ line.
//!
//! # Architecture
//!
//! This module provides the kernel-side infrastructure for MSI/MSI-X:
//!
//! - [`MsiConfig`] / [`MsixConfig`] — opaque descriptor holding the
//!   message address, data, and allocated IRQ line(s).
//! - [`allocate_msi_vectors`] — allocates one or more IRQ lines for
//!   MSI/MSI-X from the OSTD allocator.
//!
//! Actual programming of the PCI configuration space (writing the
//! Message Address / Message Data / Mask registers) is the responsibility
//! of the PCI bus driver in [`crate::drivers::pci`], which has access to
//! the device's config space BARs.
//!
//! # Safety
//!
//! The Message Address and Message Data values are architecture-specific.
//! On x86 with the local APIC, the message address is typically
//! `0xFEE0_0000` plus the destination CPU, and the message data encodes
//! the interrupt vector and delivery mode.

use crate::drivers::irq::IrqRegistration;

/// The number of MSI vectors requested by a device.
///
/// The value must be a power of two (1, 2, 4, 8, 16, or 32).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MsiVectorCount(u16);

impl MsiVectorCount {
    /// Create a new `MsiVectorCount` if `n` is a power of two ≤ 32.
    pub fn new(n: u16) -> Option<Self> {
        if n.is_power_of_two() && n <= 32 {
            Some(Self(n))
        } else {
            None
        }
    }

    /// Return the count as a `u16`.
    pub fn get(self) -> u16 {
        self.0
    }
}

/// Descriptor for a configured MSI interrupt.
///
/// The PCI driver fills this structure after programming the device's
/// MSI capability registers.
pub struct MsiConfig {
    /// Allocated IRQ registrations, one per vector.
    pub vectors: alloc::vec::Vec<IrqRegistration>,
    /// The Message Address value programmed into the device.
    pub message_address: u32,
    /// The Message Data value programmed into the device.
    pub message_data: u16,
    /// Number of allocated vectors (power of two).
    pub count: MsiVectorCount,
}

/// Descriptor for a configured MSI-X interrupt.
///
/// MSI-X supports up to 2048 independent vectors, each with its own
/// message address/data pair stored in a table in BAR space.
pub struct MsixConfig {
    /// Allocated IRQ registrations, one per vector.
    pub vectors: alloc::vec::Vec<IrqRegistration>,
    /// Per-vector message address/data pairs.
    pub entries: alloc::vec::Vec<MsixEntry>,
}

/// A single MSI-X table entry.
#[derive(Debug, Clone, Copy)]
pub struct MsixEntry {
    /// Message Address (written to the MSI-X table).
    pub message_address: u64,
    /// Message Data (written to the MSI-X table).
    pub message_data: u32,
}

/// Allocate `count` IRQ lines for MSI/MSI-X vectors.
///
/// Each vector gets a dedicated [`IrqRegistration`].  The returned vector
/// has exactly `count` entries, or an error is returned if the IRQ
/// allocator cannot satisfy the request.
pub fn allocate_msi_vectors(
    count: MsiVectorCount,
) -> Result<alloc::vec::Vec<IrqRegistration>, ostd::Error> {
    let count = count.get() as usize;
    let mut registrations = alloc::vec::Vec::with_capacity(count);
    for _ in 0..count {
        registrations.push(IrqRegistration::alloc_any(|| {})?);
    }
    Ok(registrations)
}
