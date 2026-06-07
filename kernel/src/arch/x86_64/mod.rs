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
pub mod tss;
pub mod ports;

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

        acpi::map_mmio(madt_info.local_apic_address, 4096);
        let local_apic = lapic::LocalApic::new(madt_info.local_apic_address);
        local_apic.enable();
        let lapic_id = local_apic.id();

        for i in 0..madt_info.io_apic_count {
            if let Some(entry) = &madt_info.io_apics[i] {
                acpi::map_mmio(entry.address as u64, 4096);
                let io_apic = ioapic::IoApic::new(entry.address, entry.gsi_base);
                io_apic.configure_isa_irqs(lapic_id, &madt_info.isos, madt_info.iso_count);
            }
        }

        let timer = lapic_timer::LapicTimer::calibrate(&local_apic);
        timer.start_periodic(&local_apic, 100);

        unsafe {
            lapic::LAPIC = Some(local_apic);
        }

        X86_64::enable_interrupts();
        log::info!("APIC subsystem fully initialized.");
    }
}

