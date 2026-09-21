//! CPU Interrupt Flag Management and RAII Scoped Guards (Linux-Style).
//!
//! Provides `local_irq_save`, `local_irq_restore`, `local_irq_disable`,
//! `local_irq_enable`, and `irqs_disabled` mirroring Linux interrupt conventions,
//! alongside the RAII `IrqGuard` primitive and `with_irq_lock`.

use core::marker::PhantomData;

/// RAII Guard that disables interrupts on the local CPU upon creation
/// and restores the previous interrupt flag state when dropped (`local_irq_save`).
pub struct IrqGuard {
    was_enabled: bool,
    _marker: PhantomData<*const ()>,
}

impl IrqGuard {
    /// Disables local CPU interrupts and captures the previous interrupt state.
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            was_enabled: crate::arch::disable_interrupts(),
            _marker: PhantomData,
        }
    }

    /// Creates an `IrqGuard` with explicitly specified previous interrupt state.
    #[inline(always)]
    pub fn from_state(was_enabled: bool) -> Self {
        Self {
            was_enabled,
            _marker: PhantomData,
        }
    }

    /// Returns `true` if interrupts were enabled before this guard was acquired.
    #[inline(always)]
    pub fn was_enabled(&self) -> bool {
        self.was_enabled
    }
}

impl Default for IrqGuard {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for IrqGuard {
    #[inline(always)]
    fn drop(&mut self) {
        if self.was_enabled {
            crate::arch::enable_interrupts();
        }
    }
}

/// Linux-style `local_irq_save`: disables interrupts on the local CPU and returns
/// an RAII guard restoring the previous interrupt state upon drop.
#[inline(always)]
pub fn local_irq_save() -> IrqGuard {
    IrqGuard::new()
}

/// Linux-style `local_irq_restore`: restores the local CPU interrupt flag.
#[inline(always)]
pub fn local_irq_restore(was_enabled: bool) {
    if was_enabled {
        crate::arch::enable_interrupts();
    } else {
        crate::arch::disable_interrupts();
    }
}

/// Linux-style `local_irq_disable`: disables interrupts on the calling CPU (`cli`).
#[inline(always)]
pub fn local_irq_disable() {
    crate::arch::disable_interrupts();
}

/// Linux-style `local_irq_enable`: enables interrupts on the calling CPU (`sti`).
#[inline(always)]
pub fn local_irq_enable() {
    crate::arch::enable_interrupts();
}

/// Returns `true` if interrupts are currently disabled on the calling CPU.
#[inline(always)]
pub fn irqs_disabled() -> bool {
    !crate::arch::interrupts_enabled()
}

/// Acquires an [`IrqGuard`], disabling interrupts on the calling CPU until dropped.
#[inline(always)]
pub fn irq_lock() -> IrqGuard {
    local_irq_save()
}

/// Executes a closure with interrupts disabled on the current CPU,
/// restoring the previous interrupt state upon return.
#[inline(always)]
pub fn with_irq_lock<R>(f: impl FnOnce() -> R) -> R {
    let _guard = local_irq_save();
    f()
}
