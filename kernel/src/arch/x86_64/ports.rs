use core::arch::asm;

pub struct Ports;

impl Ports {
    /// Write a byte to the specified I/O port.
    /// # Safety
    /// Writing to arbitrary I/O ports can affect hardware state or cause undefined behavior.
    pub unsafe fn outb(port: u16, val: u8) {
        // SAFETY: Caller must guarantee the port and values are correct and do not corrupt hardware state.
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") port,
                in("al") val,
                options(nomem, nostack, preserves_flags)
            );
        }
    }

    /// Read a byte from the specified I/O port.
    /// # Safety
    /// Reading from arbitrary I/O ports can affect hardware state or cause undefined behavior.
    pub unsafe fn inb(port: u16) -> u8 {
        let val: u8;
        // SAFETY: Caller must guarantee the port is correct and safe to read from.
        unsafe {
            core::arch::asm!(
                "in al, dx",
                out("al") val,
                in("dx") port,
                options(nomem, nostack, preserves_flags)
            );
        }
        val
    }
}