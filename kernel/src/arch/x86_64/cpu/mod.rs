pub mod context;
pub mod gdt;
pub mod ports;
pub mod smp;
pub mod tss;

pub fn init() {
    gdt::init();
}
