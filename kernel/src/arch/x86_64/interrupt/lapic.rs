//! Local APIC (Advanced Programmable Interrupt Controller) driver.
//!
//! The Local APIC is a per-CPU interrupt controller integrated into each
//! processor core. It handles delivery of interrupts from the I/O APIC,
//! inter-processor interrupts (IPIs), and the local APIC timer.
//!
//! This module provides an OOP-style interface for configuring and
//! interacting with the LAPIC through memory-mapped I/O registers.

use core::ptr;

/// Global Local APIC instance, initialized during `init_apic()`.
///
/// This is accessed by interrupt handlers (e.g., timer, spurious) to send EOI.
pub(crate) static mut LAPIC: Option<LocalApic> = None;

/// LAPIC register offsets from the MMIO base address.
pub const ID: usize = 0x020;
pub const VERSION: usize = 0x030;
pub const TASK_PRIORITY: usize = 0x080;
pub const EOI: usize = 0x0B0;
pub const SPURIOUS_VECTOR: usize = 0x0F0;
pub const ICR_LOW: usize = 0x300;
pub const ICR_HIGH: usize = 0x310;
pub const LVT_TIMER: usize = 0x320;
pub const TIMER_INITIAL_COUNT: usize = 0x380;
pub const TIMER_CURRENT_COUNT: usize = 0x390;
pub const TIMER_DIVIDE_CONFIG: usize = 0x3E0;

/// The spurious interrupt vector number. Must have bits [3:0] = 1111b.
const SPURIOUS_VECTOR_NUM: u32 = 0xFF;

/// Represents the Local APIC for the current processor.
///
/// Access to the LAPIC registers is performed through memory-mapped I/O
/// at the base address discovered from the ACPI MADT table.
pub struct LocalApic {
    /// Virtual base address of the LAPIC MMIO register space.
    base_virt: *mut u32,
}

// SAFETY: The LAPIC MMIO region is per-CPU and does not move.
// Access is inherently single-threaded per-core context.
unsafe impl Send for LocalApic {}
unsafe impl Sync for LocalApic {}

impl LocalApic {
    /// Create a new `LocalApic` instance from the physical base address.
    ///
    /// The physical address is converted to a virtual address using the
    /// Higher Half Direct Map (HHDM) offset provided by the bootloader.
    pub fn new(base_phys: u64) -> Self {
        let hhdm = crate::mm::hhdm_offset();
        let base_virt = (base_phys + hhdm) as *mut u32;

        Self { base_virt }
    }

    /// Enable the Local APIC.
    ///
    /// This sets the spurious interrupt vector and enables the APIC by
    /// setting bit 8 (APIC Software Enable) in the Spurious Vector Register.
    /// The Task Priority Register is also cleared to accept all interrupts.
    pub fn enable(&self) {
        // Clear the Task Priority Register to allow all interrupt priorities.
        self.write_register(TASK_PRIORITY, 0);

        // Enable the APIC: set the spurious vector and the software-enable bit (bit 8).
        self.write_register(SPURIOUS_VECTOR, SPURIOUS_VECTOR_NUM | (1 << 8));

        log::info!(
            "LAPIC enabled (ID: {}, version: {:#x}).",
            self.id(),
            self.version()
        );
    }

    /// Read the LAPIC ID from the ID register.
    pub fn id(&self) -> u32 {
        (self.read_register(ID) >> 24) & 0xFF
    }

    /// Read the LAPIC version from the Version register.
    pub fn version(&self) -> u32 {
        self.read_register(VERSION) & 0xFF
    }

    /// Signal End-Of-Interrupt to the LAPIC.
    ///
    /// This must be called at the end of every interrupt handler for
    /// LAPIC-delivered interrupts (except spurious interrupts).
    pub fn end_of_interrupt(&self) {
        self.write_register(EOI, 0);
    }

    /// Read a 32-bit value from a LAPIC MMIO register.
    fn read_register(&self, offset: usize) -> u32 {
        // SAFETY: The LAPIC MMIO region is mapped via HHDM and each register
        // is a naturally-aligned 32-bit value at a known offset.
        unsafe {
            let reg_ptr = (self.base_virt as *const u8).add(offset) as *const u32;
            ptr::read_volatile(reg_ptr)
        }
    }

    /// Write a 32-bit value to a LAPIC MMIO register.
    fn write_register(&self, offset: usize, value: u32) {
        // SAFETY: The LAPIC MMIO region is mapped via HHDM and each register
        // is a naturally-aligned 32-bit value at a known offset.
        unsafe {
            let reg_ptr = (self.base_virt as *mut u8).add(offset) as *mut u32;
            ptr::write_volatile(reg_ptr, value);
        }
    }

    // ── Timer-related register accessors (used by LapicTimer) ──

    /// Write to the LVT Timer register.
    pub(crate) fn write_lvt_timer(&self, value: u32) {
        self.write_register(LVT_TIMER, value);
    }

    /// Read the LVT Timer register.
    pub(crate) fn read_lvt_timer(&self) -> u32 {
        self.read_register(LVT_TIMER)
    }

    /// Write the timer initial count register.
    pub(crate) fn write_timer_initial_count(&self, value: u32) {
        self.write_register(TIMER_INITIAL_COUNT, value);
    }

    /// Read the timer current count register.
    pub(crate) fn read_timer_current_count(&self) -> u32 {
        self.read_register(TIMER_CURRENT_COUNT)
    }

    /// Write the timer divide configuration register.
    pub(crate) fn write_timer_divide_config(&self, value: u32) {
        self.write_register(TIMER_DIVIDE_CONFIG, value);
    }
}

/// Get a reference to the global Local APIC instance.
/// # Safety
/// Must only be called after `init_hardware()` has completed LAPIC initialization.
pub(crate) unsafe fn get_lapic() -> &'static LocalApic {
    // SAFETY: LAPIC is initialized once in init_hardware() before interrupts are enabled.
    // After initialization, it is only read (for EOI), never written.
    unsafe {
        let lapic_ptr = core::ptr::addr_of!(LAPIC);
        (*lapic_ptr).as_ref().expect("LAPIC not initialized")
    }
}
