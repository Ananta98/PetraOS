//! Low-level 8042 PS/2 Controller and Keyboard Interface
//!
//! Provides direct I/O port communication with the 8042 PS/2 controller,
//! buffer flushing, command sending, controller configuration, and device reset.

use crate::arch::ports::Ports;
use crate::device::DriverError;

pub const DATA_PORT: u16 = 0x60;
pub const STATUS_PORT: u16 = 0x64;
pub const COMMAND_PORT: u16 = 0x64;

// 8042 Status Register bits
pub const STATUS_OUTPUT_BUFFER_FULL: u8 = 1 << 0; // Bit 0: Output buffer full (can read from port 0x60)
pub const STATUS_INPUT_BUFFER_FULL: u8 = 1 << 1;  // Bit 1: Input buffer full (cannot write to port 0x60/0x64)

// 8042 Controller Commands
pub const CMD_READ_CONFIG_BYTE: u8 = 0x20;
pub const CMD_WRITE_CONFIG_BYTE: u8 = 0x60;
pub const CMD_DISABLE_SECOND_PORT: u8 = 0xA7;
pub const CMD_ENABLE_SECOND_PORT: u8 = 0xA8;
pub const CMD_DISABLE_FIRST_PORT: u8 = 0xAD;
pub const CMD_ENABLE_FIRST_PORT: u8 = 0xAE;

// Keyboard Device Commands (sent to DATA_PORT 0x60)
pub const KBD_CMD_SET_LEDS: u8 = 0xED;
pub const KBD_CMD_ENABLE_SCANNING: u8 = 0xF4;
pub const KBD_CMD_DISABLE_SCANNING: u8 = 0xF5;
pub const KBD_CMD_RESET: u8 = 0xFF;

// Keyboard Responses
pub const KBD_RESP_ACK: u8 = 0xFA;
pub const KBD_RESP_SELF_TEST_PASS: u8 = 0xAA;

const TIMEOUT_CYCLES: usize = 100_000;

pub struct Ps2Controller;

impl Ps2Controller {
    /// Wait until the controller input buffer is ready for a new byte to be written.
    pub fn wait_write() -> Result<(), DriverError> {
        for _ in 0..TIMEOUT_CYCLES {
            // SAFETY: Reading status port 0x64 is safe and has no side effects.
            let status = unsafe { Ports::inb(STATUS_PORT) };
            if (status & STATUS_INPUT_BUFFER_FULL) == 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(DriverError::WriteFailed)
    }

    /// Wait until data is available to be read from the output buffer.
    pub fn wait_read() -> Result<(), DriverError> {
        for _ in 0..TIMEOUT_CYCLES {
            // SAFETY: Reading status port 0x64 is safe and has no side effects.
            let status = unsafe { Ports::inb(STATUS_PORT) };
            if (status & STATUS_OUTPUT_BUFFER_FULL) != 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(DriverError::ReadFailed)
    }

    /// Read all pending data from the output buffer.
    pub fn flush_buffer() {
        for _ in 0..TIMEOUT_CYCLES {
            // SAFETY: Reading status port 0x64 is safe.
            let status = unsafe { Ports::inb(STATUS_PORT) };
            if (status & STATUS_OUTPUT_BUFFER_FULL) == 0 {
                break;
            }
            // SAFETY: Discarding lingering data from data port 0x60.
            unsafe {
                let _ = Ports::inb(DATA_PORT);
            }
            core::hint::spin_loop();
        }
    }

    /// Send a command byte to the 8042 controller (port 0x64).
    pub fn send_command(cmd: u8) -> Result<(), DriverError> {
        Self::wait_write()?;
        // SAFETY: Writing to controller command port 0x64 after verifying input buffer is empty.
        unsafe {
            Ports::outb(COMMAND_PORT, cmd);
        }
        Ok(())
    }

    /// Write a data byte to the 8042 data port (port 0x60).
    pub fn write_data(data: u8) -> Result<(), DriverError> {
        Self::wait_write()?;
        // SAFETY: Writing to data port 0x60 after verifying input buffer is empty.
        unsafe {
            Ports::outb(DATA_PORT, data);
        }
        Ok(())
    }

    /// Read a data byte from the 8042 data port (port 0x60).
    pub fn read_data() -> Result<u8, DriverError> {
        Self::wait_read()?;
        // SAFETY: Reading from data port 0x60 after verifying output buffer has data.
        let data = unsafe { Ports::inb(DATA_PORT) };
        Ok(data)
    }

    /// Initialize the 8042 PS/2 controller and the primary PS/2 keyboard device.
    pub fn init_keyboard() -> Result<(), DriverError> {
        // 1. Flush any existing data in the controller buffer
        Self::flush_buffer();

        // 2. Disable both PS/2 ports during configuration
        let _ = Self::send_command(CMD_DISABLE_FIRST_PORT);
        let _ = Self::send_command(CMD_DISABLE_SECOND_PORT);

        // 3. Flush buffer again
        Self::flush_buffer();

        // 4. Read Controller Configuration Byte
        Self::send_command(CMD_READ_CONFIG_BYTE)?;
        let mut config = Self::read_data()?;

        // Configure bits:
        // Bit 0 = 1: Enable First PS/2 Port Interrupt (IRQ1)
        // Bit 4 = 0: Enable First PS/2 Port Clock
        // Bit 6 = 1: Enable First PS/2 Port Translation (Scancode Set 1 compatibility)
        config |= 0x01; // Enable Port 1 Interrupt
        config &= !0x10; // Enable Port 1 Clock (0 = enabled)
        config |= 0x40; // Enable Translation to Set 1

        // Write Controller Configuration Byte back
        Self::send_command(CMD_WRITE_CONFIG_BYTE)?;
        Self::write_data(config)?;

        // 5. Enable the First PS/2 Port
        Self::send_command(CMD_ENABLE_FIRST_PORT)?;

        // 6. Reset Keyboard device
        if Self::write_data(KBD_CMD_RESET).is_ok() {
            // Read ACK (0xFA) and Self-test result (0xAA)
            let _ = Self::read_data();
            let _ = Self::read_data();
        }

        // 7. Enable keyboard scanning
        if Self::write_data(KBD_CMD_ENABLE_SCANNING).is_ok() {
            let _ = Self::read_data();
        }

        // 8. Final flush to ensure clean start
        Self::flush_buffer();

        log::info!("PS/2 Keyboard hardware initialized (IRQ1 enabled, Set 1 translation active).");
        Ok(())
    }
}
