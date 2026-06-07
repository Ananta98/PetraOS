use crate::arch::x86_64::idt::{InterruptDescriptorTable, InterruptStackFrame};
use crate::arch::x86_64::lapic_timer;
use core::arch::asm;

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

pub fn init() {
    unsafe {
        // Set up exception handlers
        IDT.entries[0].set_handler_fn(divide_by_zero_handler as *const () as u64);
        IDT.entries[3].set_handler_fn(breakpoint_handler as *const () as u64);
        // Double fault handler uses the dedicated stack in IST1 (index 1)
        IDT.entries[8].set_handler_fn(double_fault_handler as *const () as u64);
        IDT.entries[8].set_ist_index(1);

        IDT.entries[13].set_handler_fn(general_protection_fault_handler as *const () as u64);
        IDT.entries[14].set_handler_fn(page_fault_handler as *const () as u64);

        // LAPIC timer interrupt (vector 48)
        IDT.entries[lapic_timer::TIMER_VECTOR as usize]
            .set_handler_fn(timer_handler as *const () as u64);

        // Spurious interrupt (vector 0xFF)
        IDT.entries[0xFF].set_handler_fn(spurious_interrupt_handler as *const () as u64);

        let idt_ref = &*core::ptr::addr_of!(IDT);
        idt_ref.load();
    }
}

extern "x86-interrupt" fn divide_by_zero_handler(stack_frame: &mut InterruptStackFrame) {
    log::error!("EXCEPTION: DIVIDE BY ZERO\n{:#?}", stack_frame);
    loop {
        unsafe { asm!("hlt") }
    }
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: &mut InterruptStackFrame) {
    log::warn!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    _error_code: u64,
) {
    log::error!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
    loop {
        unsafe { asm!("hlt") }
    }
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "EXCEPTION: GENERAL PROTECTION FAULT (Error Code: {})\n{:#?}",
        error_code,
        stack_frame
    );
    loop {
        unsafe { asm!("hlt") }
    }
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    let cr2: u64;
    unsafe {
        asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
    }
    log::error!(
        "EXCEPTION: PAGE FAULT (Accessed Address: {:#x}, Error Code: {:?})\n{:#?}",
        cr2,
        error_code,
        stack_frame
    );
    loop {
        unsafe { asm!("hlt") }
    }
}

extern "x86-interrupt" fn timer_handler(_stack_frame: &mut InterruptStackFrame) {
    // Send End-Of-Interrupt to the Local APIC to acknowledge the timer interrupt.
    // SAFETY: The LAPIC is initialized before interrupts are enabled.
    unsafe {
        super::lapic::get_lapic().end_of_interrupt();
    }
}

extern "x86-interrupt" fn spurious_interrupt_handler(_stack_frame: &mut InterruptStackFrame) {
    // Spurious interrupts must NOT send EOI per the Intel APIC specification.
    // They occur when an interrupt is raised and then de-asserted before delivery.
    log::trace!("Spurious interrupt received.");
}
