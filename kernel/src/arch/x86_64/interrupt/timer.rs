//! Hardware Timer and Spurious Interrupt Handlers.
//!
//! Manages LAPIC timer ticks for scheduler preemption and handles spurious IRQs.

use crate::arch::idt::InterruptStackFrame;

/// LAPIC Timer interrupt handler.
pub extern "C" fn handle_timer(stack_frame: &mut InterruptStackFrame) {
    let cpu_id = unsafe { crate::arch::interrupt::lapic::get_lapic().id() };

    // SAFETY: LAPIC is initialized and must acknowledge the timer tick with an EOI.
    unsafe {
        crate::arch::interrupt::lapic::get_lapic().end_of_interrupt();
    }

    // Only allow preemption if interrupted in user space (Ring 3).
    // Arbitrary kernel preemption in Ring 0 is unsafe as kernel locks do not disable preemption.
    let is_user = (stack_frame.code_segment & 3) != 0;
    if is_user {
        crate::sched::tick(cpu_id, 10_000_000);
    }
}

/// Spurious APIC interrupt handler.
///
/// Spurious interrupts must NOT send EOI per the Intel APIC specification.
pub extern "C" fn handle_spurious(_stack_frame: &mut InterruptStackFrame) {
    log::trace!("Spurious interrupt received.");
}
