use crate::arch::x86_64::idt::{InterruptDescriptorTable, InterruptStackFrame};
use core::arch::asm;

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

pub fn init() {
    unsafe {
        // Set up exception handlers
        IDT.entries[0].set_handler_fn(divide_by_zero_handler as *const () as u64);
        IDT.entries[3].set_handler_fn(breakpoint_handler as *const () as u64);
        
        IDT.entries[8].set_handler_fn(double_fault_handler as *const () as u64);
        // Double fault handler uses the dedicated stack in IST1 (index 1)
        IDT.entries[8].set_ist_index(1);
        
        IDT.entries[13].set_handler_fn(general_protection_fault_handler as *const () as u64);
        IDT.entries[14].set_handler_fn(page_fault_handler as *const () as u64);
        
        let idt_ref = &*core::ptr::addr_of!(IDT);
        idt_ref.load();
    }
}

extern "x86-interrupt" fn divide_by_zero_handler(stack_frame: &mut InterruptStackFrame) {
    log::error!("EXCEPTION: DIVIDE BY ZERO\n{:#?}", stack_frame);
    loop { unsafe { asm!("hlt") } }
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: &mut InterruptStackFrame) {
    log::warn!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    _error_code: u64,
) {
    log::error!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
    loop { unsafe { asm!("hlt") } }
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "EXCEPTION: GENERAL PROTECTION FAULT (Error Code: {})\n{:#?}",
        error_code, stack_frame
    );
    loop { unsafe { asm!("hlt") } }
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
        cr2, error_code, stack_frame
    );
    loop { unsafe { asm!("hlt") } }
}
