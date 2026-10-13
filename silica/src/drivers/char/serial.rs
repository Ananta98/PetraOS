// SPDX-License-Identifier: GPL-2.0

//! 16550 UART Serial Driver for COM1 (0x3F8)
//!
//! Provides interrupt-driven serial input handling on ISA IRQ 4,
//! forwarding incoming characters into the TTY line discipline.

use ostd::{
    arch::{
        device::io_port::ReadWriteAccess,
        irq::{IRQ_CHIP, MappedIrqLine},
        trap::TrapFrame,
    },
    io::IoPort,
    irq::IrqLine,
};
use spin::Once;

const COM1_DATA_PORT: u16 = 0x3F8;
const COM1_IER_PORT: u16 = 0x3F9;
const COM1_FCR_PORT: u16 = 0x3FA;
const COM1_LCR_PORT: u16 = 0x3FB;
const COM1_MCR_PORT: u16 = 0x3FC;
const COM1_LSR_PORT: u16 = 0x3FD;

const ISA_SERIAL_IRQ: u8 = 4;

struct Com1Ports {
    data: IoPort<u8, ReadWriteAccess>,
    ier: IoPort<u8, ReadWriteAccess>,
    iir_fcr: IoPort<u8, ReadWriteAccess>,
    lcr: IoPort<u8, ReadWriteAccess>,
    mcr: IoPort<u8, ReadWriteAccess>,
    lsr: IoPort<u8, ReadWriteAccess>,
    msr: IoPort<u8, ReadWriteAccess>,
}

static COM1_PORTS: Once<Com1Ports> = Once::new();
static SERIAL_IRQ_LINE: Once<MappedIrqLine> = Once::new();

/// Drains any received bytes from COM1 UART hardware FIFO and delivers them to the TTY.
pub fn poll_drain_rx() {
    let Some(ports) = COM1_PORTS.get() else {
        return;
    };

    for _ in 0..64 {
        let lsr = ports.lsr.read();
        if (lsr & 0x01) == 0 {
            break;
        }

        let byte = ports.data.read();
        ostd::info!("[SERIAL] RX byte: 0x{:02X} ('{}')", byte, byte as char);
        crate::drivers::char::tty::handle_keyboard_char(byte);
    }

    // Acknowledge interrupt in IIR and clear modem status
    let _ = ports.iir_fcr.read();
    let _ = ports.msr.read();
}

fn handle_serial_irq(_frame: &TrapFrame) {
    poll_drain_rx();
}

pub fn init() -> crate::api::errno::Result<()> {
    ostd::info!("16550 Serial (COM1): Initializing input handler...");

    // Configure 16550 UART COM1
    match (
        IoPort::<u8, ReadWriteAccess>::acquire_overlapping(COM1_DATA_PORT),
        IoPort::<u8, ReadWriteAccess>::acquire_overlapping(COM1_IER_PORT),
        IoPort::<u8, ReadWriteAccess>::acquire_overlapping(COM1_FCR_PORT),
        IoPort::<u8, ReadWriteAccess>::acquire_overlapping(COM1_LCR_PORT),
        IoPort::<u8, ReadWriteAccess>::acquire_overlapping(COM1_MCR_PORT),
        IoPort::<u8, ReadWriteAccess>::acquire_overlapping(COM1_LSR_PORT),
        IoPort::<u8, ReadWriteAccess>::acquire_overlapping(0x3FE),
    ) {
        (Ok(data), Ok(ier), Ok(iir_fcr), Ok(lcr), Ok(mcr), Ok(lsr), Ok(msr)) => {
            ier.write(0x00); // Disable interrupts during setup
            lcr.write(0x80); // Enable DLAB (set baud rate divisor)
            data.write(0x01); // Set divisor to 1 (lo byte) 115200 baud
            ier.write(0x00);  // Set divisor to 0 (hi byte)
            lcr.write(0x03); // 8 bits, no parity, 1 stop bit (8N1), DLAB disabled!
            iir_fcr.write(0x07); // Enable & clear FIFO, 1-byte trigger level
            mcr.write(0x0B); // Set DTR, RTS, and OUT2 (enables interrupt gating on PC)

            // Drain any stale bytes and clear IIR/LSR/MSR before enabling interrupts
            while (lsr.read() & 0x01) != 0 {
                let _ = data.read();
            }
            let _ = iir_fcr.read();
            let _ = msr.read();

            ier.write(0x01); // Enable Received Data Available interrupt

            COM1_PORTS.call_once(|| Com1Ports {
                data,
                ier,
                iir_fcr,
                lcr,
                mcr,
                lsr,
                msr,
            });
            ostd::info!("16550 Serial: COM1 ports acquired and configured successfully");
        }
        _ => {
            ostd::warn!("16550 Serial: Failed to acquire one or more COM1 ports");
        }
    }

    // Map ISA IRQ 4
    if let Some(chip) = IRQ_CHIP.get() {
        if let Ok(irq_line) = IrqLine::alloc() {
            match chip.map_isa_pin_to(irq_line, ISA_SERIAL_IRQ) {
                Ok(mut mapped_line) => {
                    mapped_line.on_active(handle_serial_irq);
                    SERIAL_IRQ_LINE.call_once(|| mapped_line);
                    ostd::info!(
                        "16550 Serial: Mapped ISA IRQ {} successfully",
                        ISA_SERIAL_IRQ
                    );
                }
                Err(err) => {
                    ostd::warn!(
                        "16550 Serial: Failed to map ISA IRQ {}: {:?}",
                        ISA_SERIAL_IRQ,
                        err
                    );
                }
            }
        }
    }

    Ok(())
}

crate::module!(
    "16550 Serial UART COM1 Input Driver",
    "Ananta98",
    crate::modules::InitcallLevel::Device,
    init
);
