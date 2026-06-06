use crate::arch::x86_64::{inb, outb};
use super::SerialBackend;

pub struct PortIoBackend {
    base_port: u16,
}

impl PortIoBackend {
    pub const fn new(base_port: u16) -> Self {
        Self { base_port }
    }
}

impl SerialBackend for PortIoBackend {
    fn read_reg(&self, offset: u16) -> u8 {
        unsafe { inb(self.base_port + offset) }
    }

    fn write_reg(&self, offset: u16, val: u8) {
        unsafe { outb(self.base_port + offset, val) }
    }
}
