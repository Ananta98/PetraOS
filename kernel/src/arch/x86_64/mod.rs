pub mod acpi;
pub mod cpu;
pub mod interrupt;
pub mod paging;
pub mod signal;
pub mod syscall;
pub mod timer;

pub use cpu::gdt;
pub use cpu::ports;
pub use cpu::rdtsc;
pub use cpu::tss;
pub use cpu::userspace;
pub use cpu::{active_address_space_root, read_cr2, set_address_space_root};
pub use interrupt::idt;
pub use interrupt::interrupts;
pub use interrupt::lapic;
pub use paging::ArchPageTable;
pub use timer::lapic_timer;

use x86_64::instructions::hlt;
use x86_64::instructions::interrupts as x86_interrupts;

/// Enable interrupts on the calling CPU.
#[inline(always)]
pub fn enable_interrupts() {
    x86_interrupts::enable();
}

/// Halt CPU until the next interrupt.
#[inline(always)]
pub fn halt() {
    hlt();
}

pub fn idle() -> ! {
    loop {
        halt();
    }
}

/// Disable interrupts on the calling CPU and return the previous interrupt flag state.
#[inline(always)]
pub fn disable_interrupts() -> bool {
    let was_enabled = interrupts_enabled();
    x86_interrupts::disable();
    was_enabled
}

/// Check if interrupts are currently enabled on the calling CPU.
#[inline(always)]
pub fn interrupts_enabled() -> bool {
    x86_interrupts::are_enabled()
}

/// Execute a closure with interrupts disabled, restoring the previous interrupt state afterwards.
#[inline(always)]
pub fn without_interrupts<R>(func: impl FnOnce() -> R) -> R {
    x86_interrupts::without_interrupts(func)
}

/// Get the Local APIC ID of the calling CPU core (defaults to 0 if APIC not yet initialized).
pub fn cpu_id() -> u32 {
    unsafe {
        interrupt::lapic::try_get_lapic()
            .map(|l| l.id())
            .unwrap_or(0)
    }
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

    enable_interrupts();

    // Start Application Processors now that the BSP is fully online.
    cpu::smp::start_aps();
}
