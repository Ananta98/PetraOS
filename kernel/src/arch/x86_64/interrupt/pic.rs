//! Legacy 8259 PIC (Programmable Interrupt Controller) disable module.
//!
//! The 8259 PIC is the original interrupt controller on IBM PC-compatible
//! systems. When switching to the APIC, the legacy PIC must be disabled
//! to prevent spurious interrupts and conflicts with APIC-routed IRQs.
//!
//! This module remaps the PIC vectors out of the CPU exception range
//! (0-31) and then masks all 16 IRQ lines.

use crate::arch::ports::Ports;

/// I/O port addresses for the dual 8259 PIC chips.
const MASTER_COMMAND: u16 = 0x20;
const MASTER_DATA: u16 = 0x21;
const SLAVE_COMMAND: u16 = 0xA0;
const SLAVE_DATA: u16 = 0xA1;

/// ICW1 flags.
const ICW1_INIT: u8 = 0x10;
const ICW1_ICW4_NEEDED: u8 = 0x01;

/// ICW4 flags.
const ICW4_8086_MODE: u8 = 0x01;

/// Represents the dual 8259 PIC (master + slave) chip set.
///
/// This struct provides methods to remap and disable the legacy PICs,
/// which is required before enabling the APIC interrupt controllers.
pub struct LegacyPic;

impl LegacyPic {
    /// Disable the legacy 8259 PIC by remapping and masking all IRQ lines.
    ///
    /// The PICs are first remapped so their vectors start at 32 (master)
    /// and 40 (slave) to avoid conflicting with CPU exception vectors 0-31.
    /// Then all 16 IRQ lines are masked to prevent any interrupts from
    /// being delivered through the legacy PIC path.
    pub fn disable() {
        // SAFETY: These I/O port writes follow the standard 8259 PIC
        // initialization sequence documented in the Intel 8259A datasheet.
        unsafe {
            // Save current masks
            let mask_master = Ports::inb(MASTER_DATA);
            let mask_slave = Ports::inb(SLAVE_DATA);

            // ICW1: Begin initialization sequence on both PICs
            Ports::outb(MASTER_COMMAND, ICW1_INIT | ICW1_ICW4_NEEDED);
            Self::io_wait();
            Ports::outb(SLAVE_COMMAND, ICW1_INIT | ICW1_ICW4_NEEDED);
            Self::io_wait();

            // ICW2: Set vector offsets (master=32, slave=40)
            Ports::outb(MASTER_DATA, 32);
            Self::io_wait();
            Ports::outb(SLAVE_DATA, 40);
            Self::io_wait();

            // ICW3: Tell master that slave is on IRQ2, tell slave its cascade identity
            Ports::outb(MASTER_DATA, 4); // Slave on IRQ2 (bit 2)
            Self::io_wait();
            Ports::outb(SLAVE_DATA, 2); // Cascade identity = 2
            Self::io_wait();

            // ICW4: Set 8086 mode
            Ports::outb(MASTER_DATA, ICW4_8086_MODE);
            Self::io_wait();
            Ports::outb(SLAVE_DATA, ICW4_8086_MODE);
            Self::io_wait();

            // Restore saved masks (in case anything was configured)
            Ports::outb(MASTER_DATA, mask_master);
            Ports::outb(SLAVE_DATA, mask_slave);

            // Mask all IRQ lines on both PICs (0xFF = all masked)
            Ports::outb(MASTER_DATA, 0xFF);
            Ports::outb(SLAVE_DATA, 0xFF);
        }

        log::info!("Legacy PIC disabled (all IRQ lines masked).");
    }

    /// Small I/O delay used between PIC commands.
    ///
    /// Writing to port 0x80 (unused POST diagnostic port) introduces a
    /// ~1µs delay needed by older hardware between consecutive PIC writes.
    fn io_wait() {
        // SAFETY: Port 0x80 is a safe no-op diagnostic port used for I/O delays.
        unsafe {
            Ports::outb(0x80, 0);
        }
    }
}
