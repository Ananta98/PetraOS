pub mod hpet;
pub mod lapic_timer;

use crate::arch::interrupt::lapic;

/// Initialize and start the LAPIC timer and HPET for the BSP.

pub fn init() {
    let local_apic = unsafe { lapic::get_lapic() };
    let timer = super::lapic_timer::LapicTimer::calibrate(local_apic);
    timer.start_periodic(local_apic, 100);

    hpet::init();
}
