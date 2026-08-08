pub mod acpi;
pub mod cpu;
pub mod interrupt;
pub mod paging;
pub mod syscall;
pub mod timer;

pub use cpu::context_switch;
pub use cpu::gdt;
pub use cpu::idt;
pub use cpu::ports;
pub use cpu::smp;
pub use cpu::tss;

pub use interrupt::interrupts;
pub use interrupt::ioapic;
pub use interrupt::lapic;
pub use interrupt::pic;

pub use timer::hpet;
pub use timer::lapic_timer;

/// Enable interrupts on the calling CPU.
pub fn enable_interrupts() {
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

/// Halt CPU until the next interrupt.
pub fn halt() -> ! {
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

/// Disable interrupts on the calling CPU and return the previous interrupt flag state.
pub fn disable_interrupts() -> bool {
    let flags: u64;
    unsafe {
        core::arch::asm!(
            "pushfq",
            "pop {}",
            "cli",
            out(reg) flags,
            options(nomem)
        );
    }
    (flags & (1 << 9)) != 0
}

/// Get the Local APIC ID of the calling CPU core.
pub fn cpu_id() -> u32 {
    unsafe { interrupt::lapic::get_lapic().id() }
}

/// Initialize execution stack for a new thread context.
pub fn init_stack(stack: &mut [u8], entry: extern "C" fn(*mut u8), arg: *mut u8) -> u64 {
    cpu::context_switch::init_stack(stack, entry, arg)
}

/// Switch CPU stack and execution context between two threads.
pub unsafe fn switch_context(prev_rsp_ptr: *mut u64, next_rsp: u64) {
    unsafe {
        cpu::context_switch::switch_context(prev_rsp_ptr, next_rsp);
    }
}

/// Switch CPU stack context to a target thread without saving previous context.
pub unsafe fn switch_context_to(next_rsp: u64) -> ! {
    unsafe {
        cpu::context_switch::switch_context_to(next_rsp);
    }
}

/// Main architecture hardware initialization entry point.
pub fn init() {
    cpu::init();

    let madt_info =
        acpi::parse_madt().expect("Failed to parse ACPI MADT — APIC initialization requires MADT");

    log::info!(
        "MADT parsed: LAPIC base={:#x}, {} IOAPIC(s), {} ISO(s).",
        madt_info.local_apic_address,
        madt_info.io_apic_count,
        madt_info.iso_count
    );

    interrupt::init(&madt_info);
    timer::init();
    syscall::init();

    // Start Application Processors now that the BSP is fully online.
    cpu::smp::start_aps();

    // ── Register all CPUs with the global scheduler ───────────────────
    if let Some(mp) = crate::limine::MP_REQUEST.get_response() {
        let mut guard = crate::sched::scheduler::GLOBAL_SCHEDULER.lock();
        for cpu in mp.cpus() {
            let id = cpu.lapic_id;
            if guard.register_cpu(id) {
                log::info!("Scheduler: registered CPU (LAPIC ID {})", id);
            }
        }
    }

    enable_interrupts();
}
