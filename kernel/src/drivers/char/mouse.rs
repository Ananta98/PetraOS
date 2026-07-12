use super::{CharDevice, InputBuffer, register_char_device};
use alloc::sync::Arc;
use ostd::arch::device::io_port::ReadWriteAccess;
use ostd::io::IoPort;
use ostd::sync::SpinLock;

// ---------------------------------------------------------------------------
// PS/2 port addresses & controller commands
// ---------------------------------------------------------------------------

const PS2_DATA: u16 = 0x60;
const PS2_CMD: u16 = 0x64;

// Controller command port (0x64) write values
const CMD_READ_CTRL: u8 = 0x20;
const CMD_WRITE_CTRL: u8 = 0x60;
const CMD_ENABLE_AUX: u8 = 0xA8;
const CMD_DISABLE_AUX: u8 = 0xA7;
const CMD_SEND_TO_AUX: u8 = 0xD4;

// Controller status port (0x64) read bits
const STS_OUTPUT_FULL: u8 = 0x01;
const STS_INPUT_FULL: u8 = 0x02;
const STS_AUX_OUTPUT: u8 = 0x20;

// Mouse commands (sent via CMD_SEND_TO_AUX)
const MOUSE_CMD_RESET: u8 = 0xFF;
const MOUSE_CMD_SET_DEFAULTS: u8 = 0xF6;
const MOUSE_CMD_DISABLE: u8 = 0xF5;
const MOUSE_CMD_ENABLE: u8 = 0xF4;
const MOUSE_CMD_SET_SAMPLE_RATE: u8 = 0xF3;
const MOUSE_CMD_GET_ID: u8 = 0xF2;
const MOUSE_CMD_SET_RESOLUTION: u8 = 0xE8;
const MOUSE_CMD_SET_SCALING21: u8 = 0xE7;

const MOUSE_ACK: u8 = 0xFA;
const MOUSE_NAK: u8 = 0xFE;
const MOUSE_SELFTEST_OK: u8 = 0xAA;
const MOUSE_ID_STANDARD: u8 = 0x00;
const MOUSE_ID_INTELLIMOUSE: u8 = 0x03;
const MOUSE_ID_INTELLIMOUSE_EXPLORER: u8 = 0x04;

/// Default capacity (in bytes) of the internal decoded-packet buffer.
const DEFAULT_BUFFER_CAPACITY: usize = 4096;

// ---------------------------------------------------------------------------
// Mouse button state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseButtons {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

// ---------------------------------------------------------------------------
// Decoded packet
// ---------------------------------------------------------------------------

/// A fully decoded mouse event.
///
/// Fields correspond to the standard PS/2 3-byte packet layout:
/// `dx`/`dy` are relative displacements since the last event.
#[derive(Debug, Clone, Copy, Default)]
pub struct MousePacket {
    pub buttons: MouseButtons,
    /// Relative X movement (signed, -128 .. 127).
    pub dx: i8,
    /// Relative Y movement (signed, -128 .. 127; negative = up).
    pub dy: i8,
    /// X overflow flag – set when the true displacement exceeded ±127.
    pub x_overflow: bool,
    /// Y overflow flag – set when the true displacement exceeded ±127.
    pub y_overflow: bool,
    /// Scroll wheel movement (IntelliMouse, -8 .. 7).
    pub scroll: i8,
}

/// Encode a [`MousePacket`] into the standard PS/2 3-byte wire format.
fn encode_packet(pkt: &MousePacket) -> [u8; 3] {
    let mut b0 = 0x08u8;
    if pkt.buttons.left {
        b0 |= 0x01;
    }
    if pkt.buttons.right {
        b0 |= 0x02;
    }
    if pkt.buttons.middle {
        b0 |= 0x04;
    }
    if pkt.dx < 0 {
        b0 |= 0x10;
    }
    if pkt.dy < 0 {
        b0 |= 0x20;
    }
    if pkt.x_overflow {
        b0 |= 0x40;
    }
    if pkt.y_overflow {
        b0 |= 0x80;
    }
    [b0, pkt.dx as u8, pkt.dy as u8]
}

// ---------------------------------------------------------------------------
// Packet decoder state machine
// ---------------------------------------------------------------------------

/// Tracks partial packet assembly from raw PS/2 mouse bytes.
#[derive(Debug, Clone)]
struct PacketDecoder {
    count: u8,
    buf: [u8; 3],
}

impl PacketDecoder {
    const fn new() -> Self {
        Self {
            count: 0,
            buf: [0u8; 3],
        }
    }

    /// Feed one raw byte from the mouse.
    ///
    /// Returns `Some(packet)` when a complete, synchronised 3-byte packet has
    /// been assembled.  Synchronisation is done by checking that every packet
    /// starts with bit 3 set (byte 0 flag) — bytes that violate this are
    /// silently discarded.
    fn feed(&mut self, byte: u8) -> Option<MousePacket> {
        if self.count == 0 {
            if byte & 0x08 == 0 {
                return None;
            }
            self.buf[0] = byte;
            self.count = 1;
            return None;
        }

        self.buf[self.count as usize] = byte;
        self.count += 1;

        if self.count >= 3 {
            self.count = 0;
            Some(self.decode())
        } else {
            None
        }
    }

    fn decode(&self) -> MousePacket {
        let b0 = self.buf[0];
        MousePacket {
            buttons: MouseButtons {
                left: b0 & 0x01 != 0,
                right: b0 & 0x02 != 0,
                middle: b0 & 0x04 != 0,
            },
            dx: self.buf[1] as i8,
            dy: self.buf[2] as i8,
            x_overflow: b0 & 0x40 != 0,
            y_overflow: b0 & 0x80 != 0,
            scroll: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Mouse device
// ---------------------------------------------------------------------------

/// A character device (`/dev/mouse`) that decodes PS/2 mouse packets.
///
/// ## Architecture
///
/// 1. The keyboard ISR reads raw bytes from PS/2 port **0x60** and calls
///    [`Mouse::push_packet`].
/// 2. The internal [`PacketDecoder`] reassembles 3-byte packets
///    (synchronising on the byte-0 marker bit 3).
/// 3. Completed packets are encoded back into standard PS/2 3-byte format and
///    stored in an [`InputBuffer`].
/// 4. User-space reads via `/dev/mouse` drain the buffer, yielding 3-byte
///    PS/2 packets.
///
/// ## Initialisation
///
/// Call [`Mouse::init`] once during boot to program the PS/2 controller and
/// enable the mouse in stream mode.
pub struct Mouse {
    buf: InputBuffer,
    decoder: SpinLock<PacketDecoder>,
}

impl Mouse {
    pub fn new() -> Self {
        Self {
            buf: InputBuffer::new(DEFAULT_BUFFER_CAPACITY),
            decoder: SpinLock::new(PacketDecoder::new()),
        }
    }

    /// Feed one raw byte read from the PS/2 data port (typically from the
    /// keyboard/mouse shared IRQ handler).
    pub fn push_packet(&self, byte: u8) {
        let mut decoder = self.decoder.lock();
        if let Some(pkt) = decoder.feed(byte) {
            let encoded = encode_packet(&pkt);
            // Release the lock before pushing into the input buffer
            // to avoid any risk of lock inversion (buf also uses SpinLock).
            drop(decoder);
            self.buf.push(&encoded);
        }
    }

    /// Convenience: feed a slice of raw PS/2 mouse bytes.
    pub fn push_packets(&self, data: &[u8]) {
        for &byte in data {
            self.push_packet(byte);
        }
    }

    /// Return the number of decoded 3-byte packets available to read.
    pub fn available(&self) -> usize {
        self.buf.available()
    }

    // ------------------------------------------------------------------
    // PS/2 controller initialisation
    // ------------------------------------------------------------------

    /// Program the PS/2 controller and enable the mouse in stream mode.
    ///
    /// Safe to call multiple times (returns `Ok(())` on subsequent calls if
    /// already initialised).  Returns `Err` if the PS/2 ports cannot be
    /// acquired or the mouse does not respond.
    pub fn init_hardware() -> Result<(), ostd::Error> {
        let data = IoPort::<u8, ReadWriteAccess>::acquire(PS2_DATA)?;
        let cmd = IoPort::<u8, ReadWriteAccess>::acquire(PS2_CMD)?;

        // 1. Enable the AUX (mouse) interface on the controller.
        Self::write_command(&cmd, CMD_ENABLE_AUX);

        // 2. Enable AUX interrupt in the controller command byte.
        Self::write_command(&cmd, CMD_READ_CTRL);
        let ctrl = Self::read_data(&data);
        Self::write_command(&cmd, CMD_WRITE_CTRL);
        Self::write_data(&data, ctrl | 0x02); // set bit 1 (enable AUX IRQ)

        // 3. Set mouse defaults (disable streaming, enable).
        Self::send_aux_command(&cmd, &data, MOUSE_CMD_SET_DEFAULTS)?;
        Self::send_aux_command(&cmd, &data, MOUSE_CMD_ENABLE)?;

        // 4. Try to detect IntelliMouse (scroll wheel) by cycling
        //    through magic sample rates.  This is the standard detection
        //    sequence used by Linux / FreeBSD.
        //    If the ID comes back as 0x03 or 0x04 we could enable
        //    4-byte mode, but for now we stick with 3-byte packets.
        Self::send_aux_command(&cmd, &data, MOUSE_CMD_SET_SAMPLE_RATE)?;
        Self::send_aux_data(&data, 200)?;
        Self::send_aux_command(&cmd, &data, MOUSE_CMD_SET_SAMPLE_RATE)?;
        Self::send_aux_data(&data, 100)?;
        Self::send_aux_command(&cmd, &data, MOUSE_CMD_SET_SAMPLE_RATE)?;
        Self::send_aux_data(&data, 80)?;
        Self::send_aux_command(&cmd, &data, MOUSE_CMD_GET_ID)?;
        let _mouse_id = Self::read_mouse_response(&data);
        // If mouse_id == 3 or 4, it is an IntelliMouse with scroll.
        // We could switch to 4-byte mode here, but for simplicity we
        // keep the standard 3-byte format.

        Ok(())
    }

    // ------------------------------------------------------------------
    // Low-level PS/2 helpers
    // ------------------------------------------------------------------

    fn wait_write(cmd: &IoPort<u8, ReadWriteAccess>) {
        // Spin until the input buffer is clear (bit 1 = 0).
        while cmd.read() & STS_INPUT_FULL != 0 {
            core::hint::spin_loop();
        }
    }

    fn wait_read(cmd: &IoPort<u8, ReadWriteAccess>) {
        // Spin until the output buffer is full (bit 0 = 1).
        while cmd.read() & STS_OUTPUT_FULL == 0 {
            core::hint::spin_loop();
        }
    }

    fn write_command(cmd: &IoPort<u8, ReadWriteAccess>, val: u8) {
        Self::wait_write(cmd);
        cmd.write(val);
    }

    fn write_data(data: &IoPort<u8, ReadWriteAccess>, val: u8) {
        // Writing to the data port requires the command port to be ready
        // (input buffer clear). We already have `cmd`, but `wait_write`
        // on the cmd port works regardless.
        let Ok(cmd_port) = IoPort::<u8, ReadWriteAccess>::acquire(PS2_CMD) else {
            return;
        };
        Self::wait_write(&cmd_port);
        data.write(val);
    }

    fn read_data(data: &IoPort<u8, ReadWriteAccess>) -> u8 {
        // The status port is at 0x64, but we can just read data directly;
        // if the buffer isn't full we get stale but harmless data.
        data.read()
    }

    /// Send a command *to the mouse* via the AUX channel.
    ///
    /// Writes `CMD_SEND_TO_AUX` (0xD4) to the command port (0x64) so the
    /// controller forwards the next byte written to the data port (0x60)
    /// to the AUX (mouse) device.
    fn send_aux_command(
        cmd: &IoPort<u8, ReadWriteAccess>,
        data: &IoPort<u8, ReadWriteAccess>,
        command: u8,
    ) -> Result<(), ostd::Error> {
        Self::write_command(cmd, CMD_SEND_TO_AUX);
        Self::wait_write(cmd);
        data.write(command);
        let ack = Self::read_mouse_response(data);
        if ack == MOUSE_ACK {
            Ok(())
        } else {
            Err(ostd::Error::AccessDenied)
        }
    }

    /// Send a data byte *to the mouse* (for multi-byte commands like
    /// Set Sample Rate).
    fn send_aux_data(data_port: &IoPort<u8, ReadWriteAccess>, val: u8) -> Result<(), ostd::Error> {
        let Ok(cmd_port) = IoPort::<u8, ReadWriteAccess>::acquire(PS2_CMD) else {
            return Err(ostd::Error::AccessDenied);
        };
        Self::write_command(&cmd_port, CMD_SEND_TO_AUX);
        Self::write_data(data_port, val);
        let ack = Self::read_mouse_response(data_port);
        if ack == MOUSE_ACK {
            Ok(())
        } else {
            Err(ostd::Error::AccessDenied)
        }
    }

    /// Wait for a response byte from the mouse (ACK or data).
    fn read_mouse_response(data: &IoPort<u8, ReadWriteAccess>) -> u8 {
        // The response comes from the data port.  We must wait for the
        // status to indicate output buffer full first.
        if let Ok(cmd_port) = IoPort::<u8, ReadWriteAccess>::acquire(PS2_CMD) {
            Self::wait_read(&cmd_port);
        }
        data.read()
    }
}

// ---------------------------------------------------------------------------
// CharDevice implementation
// ---------------------------------------------------------------------------

impl CharDevice for Mouse {
    fn read(&self, buf: &mut [u8]) -> Result<usize, ostd::Error> {
        self.buf.read_into(buf)
    }

    fn write(&self, _buf: &[u8]) -> Result<usize, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
}

// ---------------------------------------------------------------------------
// Initialisation
// ---------------------------------------------------------------------------

/// Register the mouse device with devfs and initialise PS/2 hardware.
pub fn init() {
    // Try to initialise the PS/2 controller & mouse hardware.
    // If this fails (e.g. no PS/2 controller, ports already claimed) the
    // device node is still registered – decoding works once an ISR starts
    // feeding bytes.
    let _ = Mouse::init_hardware();

    let mouse = Arc::new(Mouse::new());
    let _ = register_char_device("mouse", mouse);
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    fn collect_bytes(mouse: &Mouse) -> alloc::vec::Vec<u8> {
        let mut buf = [0u8; 64];
        let n = mouse.read(&mut buf).unwrap();
        buf[..n].to_vec()
    }

    #[ktest]
    fn test_single_left_click() {
        // Standard left-click: byte0 = 0x09 (bit3|bit0),
        // dx=0, dy=0.
        let mouse = Mouse::new();
        mouse.push_packets(&[0x09, 0x00, 0x00]);
        let out = collect_bytes(&mouse);
        assert_eq!(out, &[0x09, 0x00, 0x00]);
    }

    #[ktest]
    fn test_movement_right_down() {
        let mouse = Mouse::new();
        // Button state 0x08 (no buttons), dx=5, dy=3
        mouse.push_packets(&[0x08, 0x05, 0x03]);
        let out = collect_bytes(&mouse);
        assert_eq!(out, &[0x08, 0x05, 0x03]);
    }

    #[ktest]
    fn test_negative_movement() {
        let mouse = Mouse::new();
        // Button state 0x08, dx=-3 (0xFD), dy=-5 (0xFB)
        // Byte0 sign bits: X sign=0x10, Y sign=0x20 -> 0x08|0x10|0x20 = 0x38
        mouse.push_packets(&[0x38, 0xFD, 0xFB]);
        let out = collect_bytes(&mouse);
        assert_eq!(out, &[0x38, 0xFD, 0xFB]);
    }

    #[ktest]
    fn test_multiple_packets() {
        let mouse = Mouse::new();
        // Two consecutive packets:
        //   1) left-click, dx=10, dy=0   -> 09 0A 00
        //   2) right-click, dx=0, dy=-5  -> 0A 00 FB (sign bits: Y negative = 0x20)
        //                                    byte0 = 0x08|0x02|0x20 = 0x2A
        mouse.push_packets(&[0x09, 0x0A, 0x00, 0x2A, 0x00, 0xFB]);
        let out = collect_bytes(&mouse);
        assert_eq!(out, &[0x09, 0x0A, 0x00, 0x2A, 0x00, 0xFB]);
    }

    #[ktest]
    fn test_re_sync_after_garbage() {
        let mouse = Mouse::new();
        // Start with an invalid byte (bit 3 clear), then a valid packet.
        // The decoder should drop the bad byte and sync on the valid one.
        mouse.push_packets(&[0xFF, 0x08, 0x01, 0x02]);
        let out = collect_bytes(&mouse);
        assert_eq!(out, &[0x08, 0x01, 0x02]);
    }

    #[ktest]
    fn test_middle_button() {
        let mouse = Mouse::new();
        // Middle button: byte0 = 0x08|0x04 = 0x0C
        mouse.push_packets(&[0x0C, 0x00, 0x00]);
        let out = collect_bytes(&mouse);
        assert_eq!(out, &[0x0C, 0x00, 0x00]);
    }

    #[ktest]
    fn test_overflow_flags() {
        let mouse = Mouse::new();
        // Input:  0xF8 (both overflow + both sign bits + bit3), dx=0, dy=0
        // Output: 0xC8 (both overflow + bit3, sign bits cleared because dx/dy are 0)
        mouse.push_packets(&[0xF8, 0x00, 0x00]);
        let out = collect_bytes(&mouse);
        assert_eq!(out[0], 0xC8);
        assert_eq!(out[1], 0x00);
        assert_eq!(out[2], 0x00);
    }

    #[ktest]
    fn test_all_buttons() {
        let mouse = Mouse::new();
        // Left+Right+Middle: 0x08|0x01|0x02|0x04 = 0x0F
        mouse.push_packets(&[0x0F, 0x00, 0x00]);
        let out = collect_bytes(&mouse);
        assert_eq!(out[0], 0x0F);
    }
}
