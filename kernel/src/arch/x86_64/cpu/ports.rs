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

    /// Write a word (16-bit) to the specified I/O port.
    pub unsafe fn outw(port: u16, val: u16) {
        unsafe {
            core::arch::asm!(
                "out dx, ax",
                in("dx") port,
                in("ax") val,
                options(nomem, nostack, preserves_flags)
            );
        }
    }

    /// Read a word (16-bit) from the specified I/O port.
    pub unsafe fn inw(port: u16) -> u16 {
        let val: u16;
        unsafe {
            core::arch::asm!(
                "in ax, dx",
                out("ax") val,
                in("dx") port,
                options(nomem, nostack, preserves_flags)
            );
        }
        val
    }

    /// Write a double word (32-bit) to the specified I/O port.
    pub unsafe fn outl(port: u16, val: u32) {
        unsafe {
            core::arch::asm!(
                "out dx, eax",
                in("dx") port,
                in("eax") val,
                options(nomem, nostack, preserves_flags)
            );
        }
    }

    /// Read a double word (32-bit) from the specified I/O port.
    pub unsafe fn inl(port: u16) -> u32 {
        let val: u32;
        unsafe {
            core::arch::asm!(
                "in eax, dx",
                out("eax") val,
                in("dx") port,
                options(nomem, nostack, preserves_flags)
            );
        }
        val
    }
}