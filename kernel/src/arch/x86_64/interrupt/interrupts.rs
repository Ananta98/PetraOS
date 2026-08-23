use crate::arch::idt::{InterruptDescriptorTable, InterruptStackFrame};
use crate::arch::lapic_timer;
use crate::arch::{halt, read_cr2, without_interrupts};
use crate::ipc::signal::SIGSEGV;
use crate::mm::{PageFaultErrorCode, VirtAddr};

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

pub fn init() {
    unsafe {
        // Set up CPU exception handlers
        IDT.entries[0].set_handler_fn(divide_by_zero_handler as *const () as u64);
        IDT.entries[1].set_handler_fn(debug_handler as *const () as u64);
        IDT.entries[2].set_handler_fn(nmi_handler as *const () as u64);
        IDT.entries[3].set_handler_fn(breakpoint_handler as *const () as u64);
        IDT.entries[4].set_handler_fn(overflow_handler as *const () as u64);
        IDT.entries[5].set_handler_fn(bound_range_handler as *const () as u64);
        IDT.entries[6].set_handler_fn(invalid_opcode_handler as *const () as u64);
        IDT.entries[7].set_handler_fn(device_not_available_handler as *const () as u64);

        // Double fault handler uses the dedicated stack in IST1 (index 1)
        IDT.entries[8].set_handler_fn(double_fault_handler as *const () as u64);
        IDT.entries[8].set_ist_index(1);

        IDT.entries[10].set_handler_fn(invalid_tss_handler as *const () as u64);
        IDT.entries[11].set_handler_fn(segment_not_present_handler as *const () as u64);
        IDT.entries[12].set_handler_fn(stack_segment_fault_handler as *const () as u64);

        IDT.entries[13].set_handler_fn(general_protection_fault_handler as *const () as u64);
        IDT.entries[14].set_handler_fn(page_fault_handler as *const () as u64);

        IDT.entries[16].set_handler_fn(x87_floating_point_handler as *const () as u64);
        IDT.entries[17].set_handler_fn(alignment_check_handler as *const () as u64);
        IDT.entries[18].set_handler_fn(machine_check_handler as *const () as u64);
        IDT.entries[19].set_handler_fn(simd_floating_point_handler as *const () as u64);
        IDT.entries[20].set_handler_fn(virtualization_exception_handler as *const () as u64);
        IDT.entries[21].set_handler_fn(control_protection_handler as *const () as u64);

        // LAPIC timer interrupt (vector 48)
        IDT.entries[lapic_timer::TIMER_VECTOR as usize]
            .set_handler_fn(timer_handler as *const () as u64);

        // Keyboard interrupt (vector 33, ISA IRQ 1)
        IDT.entries[KEYBOARD_VECTOR as usize].set_handler_fn(keyboard_handler as *const () as u64);

        // Spurious interrupt (vector 0xFF)
        IDT.entries[0xFF].set_handler_fn(spurious_interrupt_handler as *const () as u64);

        let idt_ref = &*core::ptr::addr_of!(IDT);
        idt_ref.load();
    }
}

extern "x86-interrupt" fn divide_by_zero_handler(stack_frame: &mut InterruptStackFrame) {
    log::error!("EXCEPTION: DIVIDE BY ZERO (#DE)\n{}", stack_frame);
    halt()
}

extern "x86-interrupt" fn debug_handler(stack_frame: &mut InterruptStackFrame) {
    log::warn!("EXCEPTION: DEBUG (#DB)\n{}", stack_frame);
}

extern "x86-interrupt" fn nmi_handler(stack_frame: &mut InterruptStackFrame) {
    log::error!("EXCEPTION: NON-MASKABLE INTERRUPT (#NMI)\n{}", stack_frame);
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: &mut InterruptStackFrame) {
    log::warn!("EXCEPTION: BREAKPOINT (#BP)\n{}", stack_frame);
}

extern "x86-interrupt" fn overflow_handler(stack_frame: &mut InterruptStackFrame) {
    log::error!("EXCEPTION: OVERFLOW (#OF)\n{}", stack_frame);
}

extern "x86-interrupt" fn bound_range_handler(stack_frame: &mut InterruptStackFrame) {
    log::error!("EXCEPTION: BOUND RANGE EXCEEDED (#BR)\n{}", stack_frame);
}

fn kill_user_process(sig: u8) -> ! {
    let ppid_opt = if let Some(proc_arc) = crate::proc::current_process() {
        let mut proc = proc_arc.lock();
        proc.exit(128 + sig as i32);
        proc.ppid
    } else {
        crate::proc::ProcessId(0)
    };

    if let Some(thread_arc) = crate::proc::current_thread() {
        let mut t = thread_arc.lock();
        t.state = crate::proc::ThreadState::Zombie;
        t.exit_code = Some((128 + sig as u32) as u32);
    }

    if ppid_opt.as_u64() > 0 {
        if let Some(parent_arc) = crate::proc::find_process(ppid_opt) {
            let mut parent = parent_arc.lock();
            let _ = parent.send_signal(crate::ipc::signal::SIGCHLD);
        }
    }

    loop {
        crate::sched::schedule(false);
    }
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: &mut InterruptStackFrame) {
    if (stack_frame.code_segment & 3) == 3 {
        log::warn!(
            "User process invalid opcode (#UD) at RIP {:#x}",
            stack_frame.instruction_pointer
        );
        kill_user_process(crate::ipc::signal::SIGILL);
    }
    log::error!("EXCEPTION: INVALID OPCODE (#UD)\n{}", stack_frame);
    halt();
}

extern "x86-interrupt" fn device_not_available_handler(stack_frame: &mut InterruptStackFrame) {
    log::error!("EXCEPTION: DEVICE NOT AVAILABLE (#NM)\n{}", stack_frame);
    halt();
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "EXCEPTION: DOUBLE FAULT (#DF, Error Code: {:#x})\n{}",
        error_code,
        stack_frame
    );
    halt()
}

extern "x86-interrupt" fn invalid_tss_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "EXCEPTION: INVALID TSS (#TS, Error Code: {:#x})\n{}",
        error_code,
        stack_frame
    );
    halt();
}

extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "EXCEPTION: SEGMENT NOT PRESENT (#NP, Error Code: {:#x})\n{}",
        error_code,
        stack_frame
    );
    halt();
}

extern "x86-interrupt" fn stack_segment_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "EXCEPTION: STACK SEGMENT FAULT (#SS, Error Code: {:#x})\n{}",
        error_code,
        stack_frame
    );
    halt();
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    if (stack_frame.code_segment & 3) == 3 {
        log::warn!(
            "User process general protection fault (#GP, Code {:#x}) at RIP {:#x}",
            error_code,
            stack_frame.instruction_pointer
        );
        kill_user_process(crate::ipc::signal::SIGSEGV);
    }
    log::error!(
        "EXCEPTION: GENERAL PROTECTION FAULT (#GP, Error Code: {:#x})\n{}",
        error_code,
        stack_frame
    );
    halt();
}

extern "x86-interrupt" fn x87_floating_point_handler(stack_frame: &mut InterruptStackFrame) {
    log::error!(
        "EXCEPTION: x87 FPU FLOATING POINT ERROR (#MF)\n{}",
        stack_frame
    );
    halt();
}

extern "x86-interrupt" fn alignment_check_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "EXCEPTION: ALIGNMENT CHECK (#AC, Error Code: {:#x})\n{}",
        error_code,
        stack_frame
    );
    halt();
}

extern "x86-interrupt" fn machine_check_handler(stack_frame: &mut InterruptStackFrame) {
    log::error!("EXCEPTION: MACHINE CHECK (#MC)\n{}", stack_frame);
    halt();
}

extern "x86-interrupt" fn simd_floating_point_handler(stack_frame: &mut InterruptStackFrame) {
    log::error!(
        "EXCEPTION: SIMD FLOATING POINT EXCEPTION (#XM)\n{}",
        stack_frame
    );
    halt();
}

extern "x86-interrupt" fn virtualization_exception_handler(stack_frame: &mut InterruptStackFrame) {
    log::error!("EXCEPTION: VIRTUALIZATION EXCEPTION (#VE)\n{}", stack_frame);
    halt();
}

extern "x86-interrupt" fn control_protection_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "EXCEPTION: CONTROL PROTECTION EXCEPTION (#CP, Error Code: {:#x})\n{}",
        error_code,
        stack_frame
    );
    halt();
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    let fault_virt = VirtAddr::new(read_cr2());
    let fault_code = PageFaultErrorCode::from_bits_truncate(error_code);

    let cpu_id = unsafe { super::lapic::get_lapic().id() };

    let current_thread = crate::sched::current_thread_on_cpu(cpu_id);

    if let Some(thread_arc) = current_thread {
        let thread = thread_arc.lock();
        if let Some(proc_arc) = thread.process.upgrade() {
            let proc = proc_arc.lock();
            let mut addr_space = proc.address_space.lock();
            if addr_space.handle_page_fault(fault_virt, fault_code).is_ok() {
                return;
            }
        }
    }

    if (stack_frame.code_segment & 3) == 3 || fault_virt.as_u64() <= crate::syscalls::USER_SPACE_MAX_ADDR {
        log::warn!(
            "User process page fault (SIGSEGV) at {:#x}, Error Code: {:#x} [{:?}]",
            fault_virt.as_u64(),
            error_code,
            fault_code
        );
        kill_user_process(SIGSEGV);
    }

    log::error!(
        "UNHANDLED EXCEPTION: PAGE FAULT (Fault Address: {:#x}, Error Code: {:#x} [{:?}])\n{}",
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

    crate::sched::tick(cpu_id, 10_000_000);
    crate::sched::schedule(true);
}

extern "x86-interrupt" fn spurious_interrupt_handler(_stack_frame: &mut InterruptStackFrame) {
    // Spurious interrupts must NOT send EOI per the Intel APIC specification.
    // They occur when an interrupt is raised and then de-asserted before delivery.
    log::trace!("Spurious interrupt received.");
}

extern "x86-interrupt" fn keyboard_handler(_stack_frame: &mut InterruptStackFrame) {
    // Drain pending bytes from 8042 controller output buffer.
    // Bit 0 of port 0x64 (OBF): Output buffer full
    // Bit 5 of port 0x64 (AUX): 0 = Keyboard (Port 1), 1 = Mouse (Port 2)
    loop {
        // SAFETY: Reading status port 0x64 has no side effects.
        let status = unsafe { crate::arch::ports::Ports::inb(0x64) };
        if (status & 0x01) == 0 {
            break;
        }

        // SAFETY: Reading data port 0x60 clears the 8042 output buffer.
        let byte = unsafe { crate::arch::ports::Ports::inb(0x60) };

        // Only process keyboard data (bit 5 clear). Mouse data (bit 5 set) is discarded.
        if (status & 0x20) == 0 {
            crate::drivers::char::keyboard::handle_scancode(byte);
        }
    }

    // SAFETY: LAPIC is guaranteed to be initialized and active when receiving interrupts.
    unsafe {
        super::lapic::get_lapic().end_of_interrupt();
    }
}
