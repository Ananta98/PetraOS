use super::CpuArch;

pub struct X86_64;

impl CpuArch for X86_64 {
    fn disable_interrupts() -> bool {
        let flags: u64;
        // SAFETY: Reading rflags and executing cli is required to disable interrupts.
        unsafe {
            core::arch::asm!(
                "pushfq",
                "pop {}",
                "cli",
                out(reg) flags,
                options(nomem, preserves_flags)
            );
        }
        (flags & (1 << 9)) != 0
    }

    fn enable_interrupts() {
        // SAFETY: Enabling interrupts is safe as we are in a controlled state.
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack));
        }
    }

    fn halt() {
        // SAFETY: Halting the CPU waiting for interrupt is a standard power-saving instruction.
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }

    fn init_hardware() {
        // Hardware initialization will occur here in subsequent tasks.
    }
}

/// Write a byte to the specified I/O port.
///
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
///
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
