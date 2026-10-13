pub mod cpuinfo;
pub mod power;
pub mod signal;

pub fn init() {
    power::init();
}
