use super::CpuArch;

pub mod acpi;
pub mod gdt;
pub mod idt;
pub mod interrupts;
pub mod ioapic;
pub mod lapic;
pub mod lapic_timer;
pub mod paging;
pub mod pic;
pub mod ports;
pub mod smp;
pub mod tss;
pub mod syscall;
pub mod context_switch;

pub struct X86_64;

impl CpuArch for X86_64 {
    fn disable_interrupts() -> bool {
        let flags: u64;
        // SAFETY: Reading rflags and executing cli is required to disable interrupts.
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

    fn enable_interrupts() {
        // SAFETY: Enabling interrupts is safe as we are in a controlled state.
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack));
        }
    }

    fn halt() {
        // SAFETY: Halting the CPU waiting for interrupt is a standard power-saving instruction.
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }

    fn init_hardware() {
        gdt::init();
        interrupts::init();
        
        pic::LegacyPic::disable();

        let madt_info = acpi::parse_madt()
            .expect("Failed to parse ACPI MADT — APIC initialization requires MADT");

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

        // Start Application Processors now that the BSP is fully online.
        smp::start_aps();

        // ── Register all CPUs with the global scheduler ───────────────────
        //
        // We enumerate every CPU entry reported by Limine's MP protocol.
        // We must register the CPU cores before calling init_threads so that
        // set_running_task can successfully bind the main thread to the BSP's core.
        if let Some(mp) = crate::limine::MP_REQUEST.get_response() {
            let mut guard = crate::sched::scheduler::GLOBAL_SCHEDULER.lock();
            for cpu in mp.cpus() {
                let id = cpu.lapic_id;
                if guard.register_cpu(id) {
                    log::info!("Scheduler: registered CPU (LAPIC ID {})", id);
                }
            }
        }

        // Initialize the thread subsystem for the BSP.
        crate::proc::init_threads(lapic_id);

        X86_64::enable_interrupts();
        log::info!("APIC subsystem fully initialized.");
    }

    fn cpu_id() -> u32 {
        // SAFETY: lapic is initialized before scheduling, so get_lapic() is safe to call.
        unsafe { lapic::get_lapic().id() }
    }

    fn init_stack(stack: &mut [u8], entry: extern "C" fn(*mut u8), arg: *mut u8) -> u64 {
        context_switch::init_stack(stack, entry, arg)
    }

    unsafe fn switch_context(prev_rsp_ptr: *mut u64, next_rsp: u64) {
        // SAFETY: Caller ensures prev_rsp_ptr is valid and switching stack pointers is safe.
        unsafe { context_switch::switch_context(prev_rsp_ptr, next_rsp); }
    }

    unsafe fn switch_context_to(next_rsp: u64) -> ! {
        // SAFETY: Caller ensures stack target and switching stack pointers is safe.
        unsafe { context_switch::switch_context_to(next_rsp) }
    }
}

