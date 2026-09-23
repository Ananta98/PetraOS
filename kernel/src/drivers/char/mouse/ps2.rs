//! Low-level 8042 PS/2 Auxiliary Port and Mouse Hardware Interface

use crate::arch::ports::Ports;
use crate::device::DriverError;

pub const DATA_PORT: u16 = 0x60;
pub const STATUS_PORT: u16 = 0x64;
pub const COMMAND_PORT: u16 = 0x64;

// 8042 Status Register bits
pub const STATUS_OUTPUT_BUFFER_FULL: u8 = 1 << 0; // Bit 0: Output buffer full
pub const STATUS_INPUT_BUFFER_FULL: u8 = 1 << 1;  // Bit 1: Input buffer full

// 8042 Controller Commands
pub const CMD_READ_CONFIG_BYTE: u8 = 0x20;
pub const CMD_WRITE_CONFIG_BYTE: u8 = 0x60;
pub const CMD_ENABLE_SECOND_PORT: u8 = 0xA8;
pub const CMD_WRITE_SECOND_PORT: u8 = 0xD4;

// Mouse Device Commands (prefixed with CMD_WRITE_SECOND_PORT to controller)
pub const MOUSE_CMD_SET_RESOLUTION: u8 = 0xE8;
pub const MOUSE_CMD_GET_DEVICE_ID: u8 = 0xF2;
pub const MOUSE_CMD_SET_SAMPLE_RATE: u8 = 0xF3;
pub const MOUSE_CMD_ENABLE_STREAMING: u8 = 0xF4;
pub const MOUSE_CMD_SET_DEFAULTS: u8 = 0xF6;
pub const MOUSE_CMD_RESET: u8 = 0xFF;

// Mouse Responses
pub const MOUSE_RESP_ACK: u8 = 0xFA;
pub const MOUSE_RESP_SELF_TEST_PASS: u8 = 0xAA;

const TIMEOUT_CYCLES: usize = 100_000;
const SHORT_TIMEOUT_CYCLES: usize = 10_000;

pub struct Ps2MouseController;

impl Ps2MouseController {
    /// Flush any existing data out of the 8042 controller data buffer.
    pub fn flush_buffer() {
        for _ in 0..10_000 {
            // SAFETY: Reading status port 0x64 is safe and has no side effects.
            let status = unsafe { Ports::inb(STATUS_PORT) };
            if (status & STATUS_OUTPUT_BUFFER_FULL) == 0 {
                break;
            }
            // SAFETY: Discarding lingering data from data port 0x60.
            unsafe {
                let _ = Ports::inb(DATA_PORT);
            }
        }
    }

    /// Wait until the controller input buffer is ready for a new byte to be written.
    pub fn wait_write() -> Result<(), DriverError> {
        for _ in 0..TIMEOUT_CYCLES {
            // SAFETY: Reading status port 0x64 is safe and has no side effects.
            let status = unsafe { Ports::inb(STATUS_PORT) };
            if (status & STATUS_INPUT_BUFFER_FULL) == 0 {
                return Ok(());
            }
        }
        Err(DriverError::WriteFailed)
    }

    /// Wait until data is available to be read from the output buffer.
    pub fn wait_read() -> Result<(), DriverError> {
        for _ in 0..TIMEOUT_CYCLES {
            // SAFETY: Reading status port 0x64 is safe.
            let status = unsafe { Ports::inb(STATUS_PORT) };
            if (status & STATUS_OUTPUT_BUFFER_FULL) != 0 {
                return Ok(());
            }
        }
        Err(DriverError::ReadFailed)
    }

    /// Short poll wait to drain optional trailing bytes without lengthy delays.
    pub fn wait_read_short() -> Result<(), DriverError> {
        for _ in 0..SHORT_TIMEOUT_CYCLES {
            // SAFETY: Reading status port 0x64 is safe.
            let status = unsafe { Ports::inb(STATUS_PORT) };
            if (status & STATUS_OUTPUT_BUFFER_FULL) != 0 {
                return Ok(());
            }
        }
        Err(DriverError::ReadFailed)
    }

    /// Send a command byte directly to the 8042 controller (port 0x64).
    pub fn send_controller_command(cmd: u8) -> Result<(), DriverError> {
        Self::wait_write()?;
        // SAFETY: Verified input buffer is empty before writing to command port.
        unsafe {
            Ports::outb(COMMAND_PORT, cmd);
        }
        Ok(())
    }

    /// Send a byte directly to the auxiliary device (mouse) via command 0xD4.
    pub fn write_mouse(data: u8) -> Result<(), DriverError> {
        Self::send_controller_command(CMD_WRITE_SECOND_PORT)?;
        Self::wait_write()?;
        // SAFETY: Verified input buffer is empty before writing data byte to port 0x60.
        unsafe {
            Ports::outb(DATA_PORT, data);
        }
        Ok(())
    }

    /// Read a response byte from port 0x60.
    pub fn read_response() -> Result<u8, DriverError> {
        Self::wait_read()?;
        // SAFETY: Verified output buffer is full before reading from port 0x60.
        let data = unsafe { Ports::inb(DATA_PORT) };
        Ok(data)
    }

    /// Send a command to the mouse and verify the ACK (0xFA) response.
    pub fn send_mouse_command(cmd: u8) -> Result<(), DriverError> {
        Self::write_mouse(cmd)?;
        let ack = Self::read_response()?;
        if ack == MOUSE_RESP_ACK {
            Ok(())
        } else {
            Err(DriverError::WriteFailed)
        }
    }

    /// Initialize the 8042 auxiliary port and the PS/2 mouse device.
    /// Initialize the 8042 auxiliary port and the PS/2 mouse device.
    /// Returns `true` if IntelliMouse extension (scroll wheel) was successfully enabled.
    pub fn init_mouse() -> Result<bool, DriverError> {
        // Temporarily mask IRQ 1 in the IOAPIC to prevent the keyboard ISR from
        // intercepting 8042 controller configuration bytes and mouse responses.
        crate::arch::interrupt::ioapic::mask_isa_irq(1);

        let res = Self::init_mouse_internal();

        // Always ensure Keyboard IRQ 1 is restored even if mouse initialization fails
        crate::arch::interrupt::ioapic::unmask_isa_irq(1);

        res
    }

    fn init_mouse_internal() -> Result<bool, DriverError> {
        // 1. Flush any leftover bytes in the 8042 buffer
        Self::flush_buffer();

        // 2. Enable the auxiliary PS/2 port on the 8042 controller
        Self::send_controller_command(CMD_ENABLE_SECOND_PORT)?;

        // 3. Read 8042 Controller Configuration Byte
        Self::send_controller_command(CMD_READ_CONFIG_BYTE)?;
        let mut config = Self::read_response()?;

        // Configure bits:
        // - Bit 0 = 1: Port 1 Interrupt (Keyboard) enabled
        // - Bit 1 = 0: Port 2 Interrupt (Mouse) disabled during reset/probing
        // - Bit 4 = 0: Port 1 Clock (Keyboard) enabled
        // - Bit 5 = 0: Port 2 Clock (Mouse) enabled
        // - Bit 6 = 1: Port 1 Translation (Keyboard Set 1) enabled
        config |= 0x41;  // Port 1 IRQ and Translation
        config &= !0x32; // Both clocks enabled (bits 4, 5 = 0), Port 2 IRQ disabled (bit 1 = 0)

        // Write Controller Configuration Byte back
        Self::send_controller_command(CMD_WRITE_CONFIG_BYTE)?;
        Self::wait_write()?;
        // SAFETY: Writing updated config byte back to port 0x60.
        unsafe {
            Ports::outb(DATA_PORT, config);
        }

        // 4. Reset mouse device:
        // Standard PS/2 mouse responds with:
        // - 0xFA: ACK
        // - 0xAA: Self-test passed (or 0xFC on failure)
        // - 0x00: Standard mouse Device ID
        if Self::write_mouse(MOUSE_CMD_RESET).is_ok() {
            // Read 0xFA ACK
            let _ = Self::read_response();
            // Read 0xAA self-test passed
            let _ = Self::read_response();
            // Read optional 0x00 device ID
            let _ = Self::read_response();
            // Flush any remaining trailing bytes
            Self::flush_buffer();
        }

        // 5. Set defaults (sampling rate 100, resolution 4 counts/mm)
        let _ = Self::send_mouse_command(MOUSE_CMD_SET_DEFAULTS);

        // 6. Try enabling IntelliMouse scroll wheel extension:
        // Magic sequence: sample rate 200 -> 100 -> 80
        let has_wheel = Self::try_enable_wheel().unwrap_or(false);

        // 7. Set sample rate to 100 packets/sec for smooth desktop movement
        let _ = Self::send_mouse_command(MOUSE_CMD_SET_SAMPLE_RATE);
        let _ = Self::send_mouse_command(100);

        // 8. Set resolution to 8 counts/mm for responsive tracking
        let _ = Self::send_mouse_command(MOUSE_CMD_SET_RESOLUTION);
        let _ = Self::send_mouse_command(3);

        // 9. Enable data reporting (streaming)
        if Self::send_mouse_command(MOUSE_CMD_ENABLE_STREAMING).is_err() {
            return Err(DriverError::NoDevice);
        }

        // 10. Enable Port 2 Interrupt in the 8042 Controller Configuration Byte
        // Re-use `config` without re-reading port 0x60 (which could read incoming mouse streaming bytes)
        config |= 0x02; // Bit 1 = 1: Enable Port 2 Interrupt (IRQ 12)
        config |= 0x01; // Bit 0 = 1: Ensure Port 1 Interrupt (IRQ 1) remains active
        config |= 0x40; // Bit 6 = 1: Ensure Port 1 Translation remains active
        config &= !0x30; // Bits 4, 5 = 0: Ensure both clocks remain enabled

        Self::send_controller_command(CMD_WRITE_CONFIG_BYTE)?;
        Self::wait_write()?;
        // SAFETY: Writing final config byte back to port 0x60.
        unsafe {
            Ports::outb(DATA_PORT, config);
        }

        // 11. Unmask ISA IRQ 12 (Mouse) on the IOAPIC
        crate::arch::interrupt::ioapic::unmask_isa_irq(12);

        log::info!(
            "[PS/2 Mouse] Initialized successfully (IRQ 12 enabled, IntelliMouse wheel: {}).",
            has_wheel
        );

        Ok(has_wheel)
    }

    /// Execute the IntelliMouse magic sequence to negotiate scroll wheel support.
    fn try_enable_wheel() -> Result<bool, DriverError> {
        Self::send_mouse_command(MOUSE_CMD_SET_SAMPLE_RATE)?;
        Self::send_mouse_command(200)?;

        Self::send_mouse_command(MOUSE_CMD_SET_SAMPLE_RATE)?;
        Self::send_mouse_command(100)?;

        Self::send_mouse_command(MOUSE_CMD_SET_SAMPLE_RATE)?;
        Self::send_mouse_command(80)?;

        // Query device ID
        Self::send_mouse_command(MOUSE_CMD_GET_DEVICE_ID)?;
        let device_id = Self::read_response()?;

        Ok(device_id == 3 || device_id == 4)
    }
}
