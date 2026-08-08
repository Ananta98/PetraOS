pub mod hpet;
pub mod lapic_timer;

use crate::arch::interrupt::lapic;

pub fn init() {
    let local_apic = unsafe { lapic::get_lapic() };
    let timer = lapic_timer::LapicTimer::calibrate(local_apic);
    timer.start_periodic(local_apic, 100);

    hpet::init();
}
