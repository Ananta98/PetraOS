pub mod interrupts;
pub mod ioapic;
pub mod lapic;
pub mod pic;

use crate::arch::acpi;
use crate::arch::paging;

pub fn init(madt_info: &acpi::MadtInfo) {
    interrupts::init();
    pic::LegacyPic::disable();

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

    unsafe {
        lapic::LAPIC = Some(local_apic);
    }
}
