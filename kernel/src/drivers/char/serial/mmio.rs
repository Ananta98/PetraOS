use super::SerialBackend;

pub struct MmioBackend {
    base_addr: *mut u8,
}

// SAFETY: Assuming MMIO addresses are safe to send and share across threads.
unsafe impl Send for MmioBackend {}
unsafe impl Sync for MmioBackend {}

impl MmioBackend {
    pub const fn new(base_addr: *mut u8) -> Self {
        Self { base_addr }
    }
}

impl SerialBackend for MmioBackend {
    fn read_reg(&self, offset: u16) -> u8 {
        unsafe { core::ptr::read_volatile(self.base_addr.add(offset as usize)) }
    }

    fn write_reg(&self, offset: u16, val: u8) {
        unsafe { core::ptr::write_volatile(self.base_addr.add(offset as usize), val) }
    }
}
