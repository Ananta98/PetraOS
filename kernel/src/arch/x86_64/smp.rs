//! Symmetric Multi-Processing (SMP) initialization.
//!
//! Uses the Limine MP protocol to discover and start Application Processors
//! (APs). Each AP initialises its own GDT/TSS, loads the shared IDT, enables
//! its Local APIC, and then spins in `hlt`.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::arch::CpuArch;
use crate::arch::x86_64::{gdt, interrupts, lapic, lapic_timer};

/// Count of APs that have fully completed initialisation.
static APS_ONLINE: AtomicU32 = AtomicU32::new(0);

/// Entry point jumped to by each Application Processor via Limine's
/// `goto_address` mechanism.
///
/// # Safety
/// Called directly by the Limine bootloader; no Rust runtime setup is needed
/// beyond what Limine provides (a valid stack and the `Cpu` pointer).
unsafe extern "C" fn ap_entry(cpu: &limine::mp::Cpu) -> ! {
    // ── Per-CPU hardware setup ────────────────────────────────────────────

    // Initialise this AP's own GDT and TSS.
    // SAFETY: called once per AP before any other hardware access.
    let tss_addr = gdt::init_per_cpu();
    unsafe {
        crate::arch::x86_64::tss::CPU_TSS_POINTERS[cpu.lapic_id as usize] = tss_addr;
    }

    // Load the shared IDT so exception/interrupt handlers are available.
    // SAFETY: IDT is fully initialised before any AP is released.
    unsafe { interrupts::load_idt(); }

    // Enable this AP's Local APIC.
    let lapic_phys = crate::arch::x86_64::acpi::parse_madt()
        .map(|m| m.local_apic_address)
        .unwrap_or(0xFEE0_0000);

    crate::arch::x86_64::paging::map_mmio(lapic_phys, 4096);
    let local_apic = lapic::LocalApic::new(lapic_phys);
    local_apic.enable();

    // Calibrate and start the LAPIC timer for this AP.
    let timer = lapic_timer::LapicTimer::calibrate(&local_apic);
    timer.start_periodic(&local_apic, 100);

    // Initialize the thread subsystem for this AP.
    crate::proc::init_threads(cpu.lapic_id);

    // Enable interrupts on this AP.
    crate::arch::x86_64::X86_64::enable_interrupts();

    log::info!(
        "SMP: AP online (processor_id={}, lapic_id={}).",
        cpu.id,
        cpu.lapic_id,
    );

    // Signal to the BSP that this AP is ready.
    APS_ONLINE.fetch_add(1, Ordering::Release);

    // Park this AP in a low-power halt loop.
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)); }
    }
}

/// Start all Application Processors discovered by Limine.
///
/// Must be called after the BSP has fully initialised the APIC subsystem so
/// that the shared IDT and LAPIC base address are valid.
pub fn start_aps() {
    let mp_response = match crate::limine::MP_REQUEST.get_response() {
        Some(r) => r,
        None => {
            log::warn!("SMP: no MP response from bootloader — running uniprocessor.");
            return;
        }
    };

    let cpus = mp_response.cpus();
    let bsp_lapic_id = mp_response.bsp_lapic_id();
    let total = cpus.len();

    log::info!(
        "SMP: {} CPU(s) found, BSP LAPIC ID = {}.",
        total,
        bsp_lapic_id,
    );

    if total <= 1 {
        log::info!("SMP: single-core system, no APs to start.");
        return;
    }

    let ap_count = (total - 1) as u32;

    // Release each AP (skip the BSP itself).
    for cpu in cpus {
        if cpu.lapic_id == bsp_lapic_id {
            continue;
        }
        // SAFETY: ap_entry satisfies the `extern "C" fn(*const Cpu) -> !`
        // signature required by Limine's goto_address protocol.
        cpu.goto_address.write(ap_entry);
    }

    // Busy-wait until every AP has incremented APS_ONLINE.
    // This is safe to do on the BSP because interrupts are already enabled
    // and the LAPIC timer keeps firing.
    let mut spins: u64 = 0;
    while APS_ONLINE.load(Ordering::Acquire) < ap_count {
        core::hint::spin_loop();
        spins += 1;
        if spins == 100_000_000 {
            log::warn!("SMP: timeout waiting for APs — only {} / {} online.",
                APS_ONLINE.load(Ordering::Relaxed), ap_count);
            break;
        }
    }

    log::info!(
        "SMP: {} / {} AP(s) online and ready.",
        APS_ONLINE.load(Ordering::Relaxed),
        ap_count,
    );
}
