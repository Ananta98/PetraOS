use crate::arch::halt;
use crate::arch::idt::{InterruptDescriptorTable, InterruptStackFrame};
use crate::arch::lapic_timer;

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
        IDT.entries[14].set_handler_fn(page_fault_handler as *const () as u64);

        // LAPIC timer interrupt (vector 48)
        IDT.entries[lapic_timer::TIMER_VECTOR as usize]
            .set_handler_fn(timer_handler as *const () as u64);

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
    let _access_flags = fault_code.to_generic_access();

    log::error!(
        "EXCEPTION: PAGE FAULT (Fault Address: {:#x}, Error Code: {:#x} [{:?}])\n{:#?}",
        fault_virt.as_u64(),
        error_code,
        fault_code,
        stack_frame
    );
    halt();
}

extern "x86-interrupt" fn timer_handler(_stack_frame: &mut InterruptStackFrame) {
    // ── Determine which CPU we are running on ─────────────────────────────
    //
    // SAFETY: The LAPIC is fully initialised before any AP raises its first
    // timer interrupt, so `get_lapic()` is always valid here.
    let cpu_id = unsafe { super::lapic::get_lapic().id() };

    // ── Acknowledge the interrupt ─────────────────────────────────────────
    //
    // SAFETY: EOI must be written to the LAPIC after every non-spurious
    // interrupt. The LAPIC is guaranteed to be initialised at this point.
    //
    // We send EOI before the context switch so that the LAPIC is ready to
    // accept new interrupts on the newly scheduled thread once it enables them.
    unsafe {
        super::lapic::get_lapic().end_of_interrupt();
    }

    // ── Drive the scheduler ────────────────────────────────────────────────
    //
    // Advance vruntime / RR time-slice accounting for `cpu_id` by one tick,
    // then ask the scheduler which task should run next.
    let _next = crate::sched::scheduler::tick_and_schedule(cpu_id);

    // // Perform the context switch
    // if let Some(next_id) = _next {
    //     crate::proc::switch_to(cpu_id, next_id);
    // } else {
    //     // Switch to the CPU's idle thread if no other tasks are runnable
    //     crate::proc::switch_to(
    //         cpu_id,
    //         crate::sched::sched_thread::ThreadId((cpu_id + 100) as u64),
    //     );
    // }
}

extern "x86-interrupt" fn spurious_interrupt_handler(_stack_frame: &mut InterruptStackFrame) {
    // Spurious interrupts must NOT send EOI per the Intel APIC specification.
    // They occur when an interrupt is raised and then de-asserted before delivery.
    log::trace!("Spurious interrupt received.");
}
