pub mod context_switch;
pub mod gdt;
pub mod idt;
pub mod ports;
pub mod smp;
pub mod tss;

pub fn init() {
    gdt::init();
}
