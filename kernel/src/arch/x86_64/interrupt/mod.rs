pub mod fault;
pub mod flags;
pub mod handler;
pub mod idt;
pub mod ioapic;
pub mod irq;
pub mod lapic;
pub mod pic;
pub mod timer;

use crate::arch::acpi;
use crate::arch::interrupt::ioapic::IoApic;
use crate::arch::lapic::LocalApic;
use crate::mm::map_mmio;
pub use flags::{disable_interrupts, enable_interrupts, without_interrupts};
pub use handler::KEYBOARD_VECTOR;
pub use idt::{InterruptDescriptorTable, InterruptStackFrame};
pub use irq::load_idt;

/// Initialize the interrupt subsystem.
///
/// 1. Loads all exception and IRQ stubs into the IDT.
/// 2. Disables the legacy 8259 PIC.
/// 3. Maps and enables the Local APIC.
/// 4. Configures all I/O APICs with ISA IRQ override information.
pub fn init(madt_info: &acpi::MadtInfo) {
    irq::init();
    self::pic::LegacyPic::disable();

    map_mmio(madt_info.local_apic_address, 4096);
    let local_apic = LocalApic::new(madt_info.local_apic_address);
    local_apic.enable();
    let lapic_id = local_apic.id();

    for i in 0..madt_info.io_apic_count {
        if let Some(entry) = &madt_info.io_apics[i] {
            map_mmio(entry.address as u64, 4096);
            let io_apic = IoApic::new(entry.address, entry.gsi_base);
            io_apic.configure_isa_irqs(lapic_id, &madt_info.isos, madt_info.iso_count);
            self::ioapic::register_ioapic(io_apic);
        }
    }

    self::ioapic::set_isos(&madt_info.isos, madt_info.iso_count);

    unsafe {
        self::lapic::LAPIC = Some(local_apic);
    }
}
