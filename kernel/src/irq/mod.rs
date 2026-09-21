//! IRQ Management and Interrupt Control Subsystem for PetraOS.
//!
//! Follows Linux-style interrupt conventions for CPU-local interrupt flags,
//! line management, and interrupt handler return codes (`IrqReturn`).

pub mod lock;

pub use lock::{
    irq_lock, irqs_disabled, local_irq_disable, local_irq_enable, local_irq_restore,
    local_irq_save, with_irq_lock, IrqGuard,
};

/// Linux-style interrupt handler return value (`irqreturn_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum IrqReturn {
    /// Interrupt was not from this device (unhandled).
    None = 0,
    /// Interrupt was handled by this device.
    Handled = 1,
    /// Interrupt needs bottom-half/threaded handler processing.
    WakeThread = 2,
}

/// Mask/disable an ISA IRQ line at the controller level.
#[inline(always)]
pub fn disable_irq(irq: u8) {
    crate::arch::interrupt::ioapic::mask_isa_irq(irq);
}

/// Unmask/enable an ISA IRQ line at the controller level.
#[inline(always)]
pub fn enable_irq(irq: u8) {
    crate::arch::interrupt::ioapic::unmask_isa_irq(irq);
}
