//! I/O APIC (Input/Output Advanced Programmable Interrupt Controller) driver.
//!
//! The I/O APIC receives external hardware interrupts (keyboard, disk, etc.)
//! and routes them to one or more Local APICs based on its redirection table.
//!
//! This module provides an OOP-style interface for configuring the IOAPIC's
//! redirection table entries, including masking/unmasking individual IRQs
//! and applying Interrupt Source Override (ISO) entries from the ACPI MADT.

use super::acpi::InterruptSourceOverride;
use crate::sync::Mutex;
use core::ptr;

/// IOAPIC register indices accessed via the indirect register select mechanism.
const ID: u32 = 0x00;
const VERSION: u32 = 0x01;
/// Redirection table entries start at register 0x10.
/// Each entry is 64 bits wide, occupying two consecutive 32-bit registers.
const REDIRECTION_TABLE_BASE: u32 = 0x10;

/// Delivery mode for IOAPIC redirection entries.
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum DeliveryMode {
    Fixed = 0b000,
    LowestPriority = 0b001,
    Smi = 0b010,
    Nmi = 0b100,
    Init = 0b101,
    ExtInt = 0b111,
}

/// Represents a single I/O APIC controller.
///
/// The IOAPIC uses an indirect register access scheme:
/// - Write the register index to the IOREGSEL register (offset 0x00)
/// - Read/write the value from/to the IOWIN register (offset 0x10)
#[derive(Clone, Copy)]
pub struct IoApic {
    /// Virtual address of the IOREGSEL (register select) register.
    reg_select: *mut u32,
    /// Virtual address of the IOWIN (register data window) register.
    reg_data: *mut u32,
    /// The Global System Interrupt base for this IOAPIC.
    gsi_base: u32,
}

// SAFETY: The IOAPIC MMIO region is fixed hardware and access is
// serialized through the indirect register mechanism.
unsafe impl Send for IoApic {}
unsafe impl Sync for IoApic {}

impl IoApic {
    /// Create a new `IoApic` instance from its physical base address and GSI base.
    ///
    /// The physical address is converted to a virtual address using the HHDM offset.
    pub fn new(base_phys: u32, gsi_base: u32) -> Self {
        let hhdm = crate::mm::hhdm_offset();
        let base_virt = (base_phys as u64 + hhdm) as *mut u8;

        Self {
            // SAFETY: IOREGSEL is at offset 0x00, IOWIN at offset 0x10.
            reg_select: base_virt as *mut u32,
            reg_data: unsafe { base_virt.add(0x10) } as *mut u32,
            gsi_base,
        }
    }

    /// Read the IOAPIC ID.
    pub fn id(&self) -> u32 {
        (self.read_register(ID) >> 24) & 0xF
    }

    /// Read the maximum number of redirection entries supported (0-indexed).
    pub fn max_redirection_entries(&self) -> u32 {
        ((self.read_register(VERSION) >> 16) & 0xFF) + 1
    }

    /// Route an IRQ to a specific LAPIC with the given vector and settings.
    ///
    /// # Arguments
    /// * `irq` — The IRQ number (relative to this IOAPIC's GSI base)
    /// * `vector` — The IDT vector number to deliver
    /// * `lapic_id` — The destination Local APIC ID
    /// * `delivery_mode` — How the interrupt should be delivered
    /// * `active_low` — `true` for active-low polarity, `false` for active-high
    /// * `level_triggered` — `true` for level-triggered, `false` for edge-triggered
    pub fn set_irq_route(
        &self,
        irq: u32,
        vector: u8,
        lapic_id: u32,
        delivery_mode: DeliveryMode,
        active_low: bool,
        level_triggered: bool,
    ) {
        let mut entry: u64 = vector as u64;

        // Delivery mode (bits [10:8])
        entry |= (delivery_mode as u64 & 0x7) << 8;

        // Mask bit (bit 16): set by default so the entry is written masked.
        // Callers must explicitly invoke `unmask_irq` to enable delivery.
        entry |= 1 << 16;

        // Polarity (bit 13): 0 = active high, 1 = active low
        if active_low {
            entry |= 1 << 13;
        }

        // Trigger mode (bit 15): 0 = edge, 1 = level
        if level_triggered {
            entry |= 1 << 15;
        }

        // Destination LAPIC ID (bits [63:56])
        entry |= (lapic_id as u64 & 0xFF) << 56;

        self.write_redirection_entry(irq, entry);
    }

    /// Mask (disable) a specific IRQ in the redirection table.
    pub fn mask_irq(&self, irq: u32) {
        let entry = self.read_redirection_entry(irq);
        self.write_redirection_entry(irq, entry | (1 << 16));
    }

    /// Unmask (enable) a specific IRQ in the redirection table.
    pub fn unmask_irq(&self, irq: u32) {
        let entry = self.read_redirection_entry(irq);
        self.write_redirection_entry(irq, entry & !(1 << 16));
    }

    /// Configure the standard ISA IRQ routes (IRQs 0–15) mapped to
    /// IDT vectors 32–47, applying any Interrupt Source Overrides from the MADT.
    ///
    /// All ISA IRQs are initially masked. Only specific IRQs should be
    /// unmasked by their respective drivers after handler registration.
    pub fn configure_isa_irqs(
        &self,
        lapic_id: u32,
        isos: &[Option<InterruptSourceOverride>],
        iso_count: usize,
    ) {
        let max_entries = self.max_redirection_entries();

        for irq in 0u8..16 {
            // Check if there's an Interrupt Source Override for this ISA IRQ
            let mut gsi = irq as u32;
            let mut active_low = false;
            let mut level_triggered = false;

            for i in 0..iso_count {
                if let Some(iso) = &isos[i] {
                    if iso.irq_source == irq {
                        gsi = iso.gsi;

                        // Polarity flags (bits [1:0])
                        let polarity = iso.flags & 0x3;
                        if polarity == 3 {
                            active_low = true;
                        }

                        // Trigger mode flags (bits [3:2])
                        let trigger = (iso.flags >> 2) & 0x3;
                        if trigger == 3 {
                            level_triggered = true;
                        }

                        break;
                    }
                }
            }

            // Skip if the GSI is beyond this IOAPIC's range
            if gsi < self.gsi_base || gsi - self.gsi_base >= max_entries {
                continue;
            }

            let ioapic_irq = gsi - self.gsi_base;
            let vector = 32 + irq;

            // Route but keep masked — drivers will unmask as needed
            self.set_irq_route(
                ioapic_irq,
                vector,
                lapic_id,
                DeliveryMode::Fixed,
                active_low,
                level_triggered,
            );
            self.mask_irq(ioapic_irq);
        }

        log::info!(
            "IOAPIC initialized (ID: {}, max redirections: {}, GSI base: {}).",
            self.id(),
            max_entries,
            self.gsi_base
        );
    }

    /// Read a 64-bit redirection table entry for the given IRQ.
    fn read_redirection_entry(&self, irq: u32) -> u64 {
        let reg_low = REDIRECTION_TABLE_BASE + irq * 2;
        let reg_high = reg_low + 1;

        let low = self.read_register(reg_low) as u64;
        let high = self.read_register(reg_high) as u64;

        low | (high << 32)
    }

    /// Write a 64-bit redirection table entry for the given IRQ.
    fn write_redirection_entry(&self, irq: u32, entry: u64) {
        let reg_low = REDIRECTION_TABLE_BASE + irq * 2;
        let reg_high = reg_low + 1;

        self.write_register(reg_low, entry as u32);
        self.write_register(reg_high, (entry >> 32) as u32);
    }

    /// Read a 32-bit IOAPIC register using the indirect access mechanism.
    fn read_register(&self, reg: u32) -> u32 {
        // SAFETY: Writing to IOREGSEL selects the register, reading IOWIN returns its value.
        unsafe {
            ptr::write_volatile(self.reg_select, reg);
            ptr::read_volatile(self.reg_data)
        }
    }

    /// Write a 32-bit value to an IOAPIC register using the indirect access mechanism.
    fn write_register(&self, reg: u32, value: u32) {
        // SAFETY: Writing to IOREGSEL selects the register, writing IOWIN sets its value.
        unsafe {
            ptr::write_volatile(self.reg_select, reg);
            ptr::write_volatile(self.reg_data, value);
        }
    }
}

static IO_APICS: Mutex<[Option<IoApic>; 8]> =
    Mutex::new([None, None, None, None, None, None, None, None]);
static ISOS: Mutex<([Option<InterruptSourceOverride>; 16], usize)> = Mutex::new(([None; 16], 0));

/// Register an initialized IOAPIC instance for global IRQ routing management.
pub fn register_ioapic(ioapic: IoApic) {
    let mut guard = IO_APICS.lock();
    for slot in guard.iter_mut() {
        if slot.is_none() {
            *slot = Some(ioapic);
            break;
        }
    }
}

/// Store ACPI MADT Interrupt Source Overrides.
pub fn set_isos(isos: &[Option<InterruptSourceOverride>], count: usize) {
    let mut guard = ISOS.lock();
    guard.1 = count.min(16);
    for (i, iso) in isos.iter().take(16).enumerate() {
        guard.0[i] = *iso;
    }
}

/// Unmask a standard ISA IRQ line (0..15), resolving any MADT GSI overrides.
pub fn unmask_isa_irq(irq: u8) {
    let gsi = {
        let (isos, count) = *ISOS.lock();
        let mut target_gsi = irq as u32;
        for i in 0..count {
            if let Some(iso) = &isos[i] {
                if iso.irq_source == irq {
                    target_gsi = iso.gsi;
                    break;
                }
            }
        }
        target_gsi
    };

    unmask_gsi(gsi);
}

/// Mask a standard ISA IRQ line (0..15), resolving any MADT GSI overrides.
pub fn mask_isa_irq(irq: u8) {
    let gsi = {
        let (isos, count) = *ISOS.lock();
        let mut target_gsi = irq as u32;
        for i in 0..count {
            if let Some(iso) = &isos[i] {
                if iso.irq_source == irq {
                    target_gsi = iso.gsi;
                    break;
                }
            }
        }
        target_gsi
    };

    mask_gsi(gsi);
}

/// Unmask a Global System Interrupt (GSI) on its controlling IOAPIC.
pub fn unmask_gsi(gsi: u32) {
    let guard = IO_APICS.lock();
    for ioapic_opt in guard.iter() {
        if let Some(ioapic) = ioapic_opt {
            let max_entries = ioapic.max_redirection_entries();
            if gsi >= ioapic.gsi_base && gsi - ioapic.gsi_base < max_entries {
                let irq_line = gsi - ioapic.gsi_base;
                ioapic.unmask_irq(irq_line);
                log::info!("IOAPIC unmasked GSI {} (IRQ line {})", gsi, irq_line);
                return;
            }
        }
    }
    log::warn!("IOAPIC: No controller found for GSI {}", gsi);
}

/// Mask a Global System Interrupt (GSI) on its controlling IOAPIC.
pub fn mask_gsi(gsi: u32) {
    let guard = IO_APICS.lock();
    for ioapic_opt in guard.iter() {
        if let Some(ioapic) = ioapic_opt {
            let max_entries = ioapic.max_redirection_entries();
            if gsi >= ioapic.gsi_base && gsi - ioapic.gsi_base < max_entries {
                let irq_line = gsi - ioapic.gsi_base;
                ioapic.mask_irq(irq_line);
                log::info!("IOAPIC masked GSI {} (IRQ line {})", gsi, irq_line);
                return;
            }
        }
    }
    log::warn!("IOAPIC: No controller found for GSI {}", gsi);
}
