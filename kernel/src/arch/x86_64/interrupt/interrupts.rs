use crate::arch::halt;
use crate::arch::idt::{InterruptDescriptorTable, InterruptStackFrame};
use crate::arch::lapic_timer;

pub const KEYBOARD_VECTOR: u8 = 33;

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

/// Load the shared IDT on the calling CPU.
///
/// # Safety
/// Must only be called after `init()` has configured the IDT entries.
pub(crate) unsafe fn load_idt() {
    let idt_ref = unsafe { &*core::ptr::addr_of!(IDT) };
    idt_ref.load();
}

unsafe extern "C" {
    fn syscall_asm_entry();
}

pub fn init() {
    unsafe {
        // Set up exception handlers
        IDT.entries[0].set_handler_fn(divide_by_zero_handler as *const () as u64);
        IDT.entries[3].set_handler_fn(breakpoint_handler as *const () as u64);
        // Double fault handler uses the dedicated stack in IST1 (index 1)
        IDT.entries[8].set_handler_fn(double_fault_handler as *const () as u64);
        IDT.entries[8].set_ist_index(1);

        IDT.entries[13].set_handler_fn(general_protection_fault_handler as *const () as u64);
        IDT.entries[13].set_ist_index(2);
        IDT.entries[14].set_handler_fn(page_fault_handler as *const () as u64);
        IDT.entries[14].set_ist_index(2);

        // LAPIC timer interrupt (vector 48)
        IDT.entries[lapic_timer::TIMER_VECTOR as usize]
            .set_handler_fn(timer_handler as *const () as u64);

        // Keyboard interrupt (vector 33, ISA IRQ 1)
        IDT.entries[KEYBOARD_VECTOR as usize].set_handler_fn(keyboard_handler as *const () as u64);

        // System call interrupt (vector 0x80)
        IDT.entries[0x80].set_user_handler_fn(syscall_asm_entry as *const () as u64);

        // Spurious interrupt (vector 0xFF)
        IDT.entries[0xFF].set_handler_fn(spurious_interrupt_handler as *const () as u64);

        let idt_ref = &*core::ptr::addr_of!(IDT);
        idt_ref.load();
    }
}

extern "x86-interrupt" fn divide_by_zero_handler(stack_frame: &mut InterruptStackFrame) {
    log::error!("EXCEPTION: DIVIDE BY ZERO\n{:#?}", stack_frame);
    halt()
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: &mut InterruptStackFrame) {
    log::warn!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    _error_code: u64,
) {
    log::error!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
    halt()
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
    halt();
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    let fault_virt = unsafe { super::paging::read_cr2() };
    let fault_code = super::paging::ArchPageFaultErrorCode::from_raw(error_code);
    let access_flags = fault_code.to_generic_access();

    let cpu_id = unsafe { super::lapic::get_lapic().id() };

    let current_thread = crate::arch::without_interrupts(|| {
        let sched = crate::sched::SCHEDULER.lock();
        sched.current_threads[cpu_id as usize].clone()
    });

    if let Some(thread_arc) = current_thread {
        let thread = thread_arc.lock();
        if let Some(proc_arc) = thread.process.upgrade() {
            let proc = proc_arc.lock();
            let mut addr_space = proc.address_space.lock();
            if addr_space
                .handle_page_fault(fault_virt, access_flags)
                .is_ok()
            {
                return;
            }
        }
    }

    log::error!(
        "UNHANDLED EXCEPTION: PAGE FAULT (Fault Address: {:#x}, Error Code: {:#x} [{:?}])\n{:#?}",
        fault_virt.as_u64(),
        error_code,
        fault_code,
        stack_frame
    );
    halt();
}

extern "x86-interrupt" fn timer_handler(_stack_frame: &mut InterruptStackFrame) {
    let cpu_id = unsafe { super::lapic::get_lapic().id() };

    unsafe {
        super::lapic::get_lapic().end_of_interrupt();
    }

    crate::sched::SCHEDULER.lock().tick(cpu_id, 10_000_000);
    crate::sched::schedule(true);
}

extern "x86-interrupt" fn spurious_interrupt_handler(_stack_frame: &mut InterruptStackFrame) {
    // Spurious interrupts must NOT send EOI per the Intel APIC specification.
    // They occur when an interrupt is raised and then de-asserted before delivery.
    log::trace!("Spurious interrupt received.");
}

extern "x86-interrupt" fn keyboard_handler(_stack_frame: &mut InterruptStackFrame) {
    // SAFETY: Reading port 0x60 reads the keyboard scancode and clears the 8042 output buffer.
    let scancode = unsafe { crate::arch::ports::Ports::inb(0x60) };

    // Dispatch scancode to character keyboard driver
    crate::drivers::char::keyboard::handle_scancode(scancode);

    // SAFETY: LAPIC is guaranteed to be initialized and active when receiving interrupts.
    unsafe {
        super::lapic::get_lapic().end_of_interrupt();
    }
}
