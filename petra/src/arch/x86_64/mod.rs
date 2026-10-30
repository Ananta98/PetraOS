pub mod cpuinfo;
pub mod power;
pub mod signal;
pub mod vdso;

pub fn init() {
    power::init();
    vdso::init();
}
