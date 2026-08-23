//! Hardware Port I/O Instructions for x86_64 Architecture.
//!
//! Provides raw byte, word, and double-word input/output instructions via `in` and `out`.

use core::arch::asm;

pub struct Ports;

impl Ports {
    /// Write a byte to the specified I/O port.
    ///
    /// # Safety
    /// Writing to arbitrary I/O ports can affect hardware state or cause undefined behavior.
    #[inline(always)]
    pub unsafe fn outb(port: u16, val: u8) {
        // SAFETY: Direct hardware port write.
        unsafe {
            asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
        }
    }

    /// Read a byte from the specified I/O port.
    ///
    /// # Safety
    /// Reading from arbitrary I/O ports can affect hardware state or cause undefined behavior.
    #[inline(always)]
    pub unsafe fn inb(port: u16) -> u8 {
        let val: u8;
        // SAFETY: Direct hardware port read.
        unsafe {
            asm!("in al, dx", out("al") val, in("dx") port, options(nomem, nostack, preserves_flags));
        }
        val
    }

    /// Write a word (16-bit) to the specified I/O port.
    ///
    /// # Safety
    /// Writing to arbitrary I/O ports can affect hardware state or cause undefined behavior.
    #[inline(always)]
    pub unsafe fn outw(port: u16, val: u16) {
        // SAFETY: Direct hardware port write.
        unsafe {
            asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags));
        }
    }

    /// Read a word (16-bit) from the specified I/O port.
    ///
    /// # Safety
    /// Reading from arbitrary I/O ports can affect hardware state or cause undefined behavior.
    #[inline(always)]
    pub unsafe fn inw(port: u16) -> u16 {
        let val: u16;
        // SAFETY: Direct hardware port read.
        unsafe {
            asm!("in ax, dx", out("ax") val, in("dx") port, options(nomem, nostack, preserves_flags));
        }
        val
    }

    /// Write a double word (32-bit) to the specified I/O port.
    ///
    /// # Safety
    /// Writing to arbitrary I/O ports can affect hardware state or cause undefined behavior.
    #[inline(always)]
    pub unsafe fn outl(port: u16, val: u32) {
        // SAFETY: Direct hardware port write.
        unsafe {
            asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags));
        }
    }

    /// Read a double word (32-bit) from the specified I/O port.
    ///
    /// # Safety
    /// Reading from arbitrary I/O ports can affect hardware state or cause undefined behavior.
    #[inline(always)]
    pub unsafe fn inl(port: u16) -> u32 {
        let val: u32;
        // SAFETY: Direct hardware port read.
        unsafe {
            asm!("in eax, dx", out("eax") val, in("dx") port, options(nomem, nostack, preserves_flags));
        }
        val
    }

    /// Wait for an I/O operation to complete by writing to an unused port (0x80).
    #[inline(always)]
    pub unsafe fn io_wait() {
        // SAFETY: Port 0x80 is standard diagnostic/POST port on x86 PCs, harmless to write.
        unsafe {
            Self::outb(0x80, 0);
        }
    }
}
