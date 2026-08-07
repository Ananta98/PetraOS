pub mod acpi;
pub mod context_switch;
pub mod gdt;
pub mod hpet;
pub mod idt;
pub mod interrupts;
pub mod ioapic;
pub mod lapic;
pub mod lapic_timer;
pub mod paging;
pub mod pic;
pub mod ports;
pub mod smp;
pub mod syscall;
pub mod tss;

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
    unsafe { lapic::get_lapic().id() }
}

/// Initialize execution stack for a new thread context.
pub fn init_stack(stack: &mut [u8], entry: extern "C" fn(*mut u8), arg: *mut u8) -> u64 {
    context_switch::init_stack(stack, entry, arg)
}

/// Switch CPU stack and execution context between two threads.
pub unsafe fn switch_context(prev_rsp_ptr: *mut u64, next_rsp: u64) {
    unsafe {
        context_switch::switch_context(prev_rsp_ptr, next_rsp);
    }
}

/// Switch CPU stack context to a target thread without saving previous context.
pub unsafe fn switch_context_to(next_rsp: u64) -> ! {
    unsafe {
        context_switch::switch_context_to(next_rsp);
    }
}

/// Main architecture hardware initialization entry point.
pub fn init() {
    gdt::init();
    interrupts::init();
    pic::LegacyPic::disable();

    let madt_info =
        acpi::parse_madt().expect("Failed to parse ACPI MADT — APIC initialization requires MADT");

    log::info!(
        "MADT parsed: LAPIC base={:#x}, {} IOAPIC(s), {} ISO(s).",
        madt_info.local_apic_address,
        madt_info.io_apic_count,
        madt_info.iso_count
    );

    paging::map_mmio(madt_info.local_apic_address, 4096);
    let local_apic = lapic::LocalApic::new(madt_info.local_apic_address);
    local_apic.enable();
    let lapic_id = local_apic.id();

    for i in 0..madt_info.io_apic_count {
        if let Some(entry) = &madt_info.io_apics[i] {
            paging::map_mmio(entry.address as u64, 4096);
            let io_apic = ioapic::IoApic::new(entry.address, entry.gsi_base);
            io_apic.configure_isa_irqs(lapic_id, &madt_info.isos, madt_info.iso_count);
        }
    }

    let timer = lapic_timer::LapicTimer::calibrate(&local_apic);
    timer.start_periodic(&local_apic, 100);

    unsafe {
        lapic::LAPIC = Some(local_apic);
    }

    // Initialize High Precision Event Timer (HPET)
    hpet::init();

    // Start Application Processors now that the BSP is fully online.
    smp::start_aps();

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
