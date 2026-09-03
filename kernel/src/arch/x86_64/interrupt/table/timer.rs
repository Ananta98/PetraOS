//! Hardware Timer and Spurious Interrupt Handlers.
//!
//! Manages LAPIC timer ticks for scheduler preemption and handles spurious IRQs.

use crate::arch::idt::InterruptStackFrame;

/// LAPIC Timer interrupt handler.
pub extern "C" fn timer_handler(_stack_frame: &mut InterruptStackFrame) {
    let cpu_id = unsafe { crate::arch::interrupt::lapic::get_lapic().id() };

    // SAFETY: LAPIC is initialized and must acknowledge the timer tick with an EOI.
    unsafe {
        crate::arch::interrupt::lapic::get_lapic().end_of_interrupt();
    }

    crate::sched::tick(cpu_id, 10_000_000);
    crate::sched::schedule(true);
}

/// Spurious APIC interrupt handler.
pub extern "C" fn spurious_interrupt_handler(_stack_frame: &mut InterruptStackFrame) {
    // Spurious interrupts must NOT send EOI per the Intel APIC specification.
    // They occur when an interrupt is raised and then de-asserted before delivery.
    log::trace!("Spurious interrupt received.");
}
