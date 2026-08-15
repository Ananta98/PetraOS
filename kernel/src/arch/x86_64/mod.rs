pub mod acpi;
pub mod cpu;
pub mod interrupt;
pub mod paging;
pub mod signal;
pub mod syscall;
pub mod timer;

pub use cpu::gdt;
pub use cpu::ports;
pub use cpu::tss;
pub use cpu::userspace;
pub use interrupt::idt;

pub use interrupt::interrupts;
pub use interrupt::lapic;
pub use timer::lapic_timer;

/// Enable interrupts on the calling CPU.
pub fn enable_interrupts() {
    // SAFETY: Enabling interrupts via 'sti' is a standard CPU control operation.
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

/// Halt CPU until the next interrupt.
pub fn halt() {
    // SAFETY: Halting the CPU via 'hlt' until an interrupt arrives is a safe low-power state.
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack));
    }
}

pub fn idle() -> ! {
    loop {
        halt();
    }
}

/// Disable interrupts on the calling CPU and return the previous interrupt flag state.
pub fn disable_interrupts() -> bool {
    let flags: u64;
    // SAFETY: pushfq/pop reads RFLAGS and cli clears IF without corrupting memory.
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

/// Check if interrupts are currently enabled on the calling CPU.
#[inline(always)]
pub fn interrupts_enabled() -> bool {
    let flags: u64;
    // SAFETY: pushfq/pop reads RFLAGS without modifying state or corrupting memory.
    unsafe {
        core::arch::asm!(
            "pushfq",
            "pop {}",
            out(reg) flags,
            options(nomem)
        );
    }
    (flags & (1 << 9)) != 0
}

/// Execute a closure with interrupts disabled, restoring the previous interrupt state afterwards.
#[inline(always)]
pub fn without_interrupts<R>(func: impl FnOnce() -> R) -> R {
    let enabled = disable_interrupts();
    let result = func();
    if enabled {
        enable_interrupts();
    }
    result
}

/// Get the Local APIC ID of the calling CPU core.
pub fn cpu_id() -> u32 {
    // SAFETY: LAPIC base address is guaranteed to be mapped and initialized before this is called.
    unsafe { interrupt::lapic::get_lapic().id() }
}

/// Initialize execution stack for a new thread context.
pub fn init_stack(stack: &mut [u8], entry: extern "C" fn(*mut u8), arg: *mut u8) -> u64 {
    cpu::stack::init_stack(stack, entry, arg)
}

/// Switch CPU stack and execution context between two threads.
pub unsafe fn switch_context(prev_rsp_ptr: *mut u64, next_rsp: u64) {
    unsafe {
        cpu::context::switch_context(prev_rsp_ptr, next_rsp);
    }
}

/// Switch CPU stack context to a target thread without saving previous context.
pub unsafe fn switch_context_to(next_rsp: u64) -> ! {
    unsafe {
        cpu::context::switch_context_to(next_rsp);
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

    enable_interrupts();

    // Start Application Processors now that the BSP is fully online.
    cpu::smp::start_aps();
}
