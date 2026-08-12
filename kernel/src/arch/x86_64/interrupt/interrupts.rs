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
    let access_flags = fault_code.to_generic_access();

    let cpu_id = unsafe { super::lapic::get_lapic().id() };

    let current_thread = {
        let saved_flags = crate::arch::disable_interrupts();
        let sched = crate::sched::SCHEDULER.lock();
        let current = sched.current_threads[cpu_id as usize].clone();
        drop(sched);
        if saved_flags {
            crate::arch::enable_interrupts();
        }
        current
    };

    if let Some(thread_arc) = current_thread {
        let thread = thread_arc.lock();
        if let Some(proc_arc) = thread.process.upgrade() {
            let mut proc = proc_arc.lock();
            if let Some(addr_space) = alloc::sync::Arc::get_mut(&mut proc.address_space) {
                if addr_space.handle_page_fault(fault_virt, access_flags).is_ok() {
                    return;
                }
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

    // Advance vruntime for `cpu_id` by one tick (10ms = 10,000,000 ns),
    // then ask the scheduler which task should run next.
    crate::sched::SCHEDULER.lock().tick(cpu_id, 10_000_000);
    crate::sched::schedule(true);
}

extern "x86-interrupt" fn spurious_interrupt_handler(_stack_frame: &mut InterruptStackFrame) {
    // Spurious interrupts must NOT send EOI per the Intel APIC specification.
    // They occur when an interrupt is raised and then de-asserted before delivery.
    log::trace!("Spurious interrupt received.");
}
