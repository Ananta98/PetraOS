use x86_64::instructions::port::Port;

pub struct Ports;

impl Ports {
    /// Write a byte to the specified I/O port.
    ///
    /// # Safety
    /// Writing to arbitrary I/O ports can affect hardware state or cause undefined behavior.
    #[inline(always)]
    pub unsafe fn outb(port: u16, val: u8) {
        // SAFETY: Delegated to x86_64 Port abstraction under caller's safety contract.
        unsafe {
            let mut p = Port::<u8>::new(port);
            p.write(val);
        }
    }

    /// Read a byte from the specified I/O port.
    ///
    /// # Safety
    /// Reading from arbitrary I/O ports can affect hardware state or cause undefined behavior.
    #[inline(always)]
    pub unsafe fn inb(port: u16) -> u8 {
        // SAFETY: Delegated to x86_64 Port abstraction under caller's safety contract.
        unsafe {
            let mut p = Port::<u8>::new(port);
            p.read()
        }
    }

    /// Write a word (16-bit) to the specified I/O port.
    ///
    /// # Safety
    /// Writing to arbitrary I/O ports can affect hardware state or cause undefined behavior.
    #[inline(always)]
    pub unsafe fn outw(port: u16, val: u16) {
        // SAFETY: Delegated to x86_64 Port abstraction under caller's safety contract.
        unsafe {
            let mut p = Port::<u16>::new(port);
            p.write(val);
        }
    }

    /// Read a word (16-bit) from the specified I/O port.
    ///
    /// # Safety
    /// Reading from arbitrary I/O ports can affect hardware state or cause undefined behavior.
    #[inline(always)]
    pub unsafe fn inw(port: u16) -> u16 {
        // SAFETY: Delegated to x86_64 Port abstraction under caller's safety contract.
        unsafe {
            let mut p = Port::<u16>::new(port);
            p.read()
        }
    }

    /// Write a double word (32-bit) to the specified I/O port.
    ///
    /// # Safety
    /// Writing to arbitrary I/O ports can affect hardware state or cause undefined behavior.
    #[inline(always)]
    pub unsafe fn outl(port: u16, val: u32) {
        // SAFETY: Delegated to x86_64 Port abstraction under caller's safety contract.
        unsafe {
            let mut p = Port::<u32>::new(port);
            p.write(val);
        }
    }

    /// Read a double word (32-bit) from the specified I/O port.
    ///
    /// # Safety
    /// Reading from arbitrary I/O ports can affect hardware state or cause undefined behavior.
    #[inline(always)]
    pub unsafe fn inl(port: u16) -> u32 {
        // SAFETY: Delegated to x86_64 Port abstraction under caller's safety contract.
        unsafe {
            let mut p = Port::<u32>::new(port);
            p.read()
        }
    }
}
