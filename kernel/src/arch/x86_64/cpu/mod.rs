pub mod context;
pub mod gdt;
pub mod ports;
pub mod smp;
pub mod stack;
pub mod tss;

pub fn init() {
    gdt::init();
}
