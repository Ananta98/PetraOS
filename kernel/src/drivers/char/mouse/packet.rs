//! PS/2 Mouse Packet Parser and State Machine
//!
//! Handles standard 3-byte PS/2 mouse packets and 4-byte IntelliMouse
//! (scroll wheel + extra buttons) packet framing and sign extension.

/// A fully decoded relative mouse event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MousePacket {
    pub left_button: bool,
    pub right_button: bool,
    pub middle_button: bool,
    pub dx: i16,
    pub dy: i16,
    pub dz: i8, // Scroll wheel delta
}

/// State machine for assembling streamed mouse bytes into packets.
pub struct MousePacketParser {
    buffer: [u8; 4],
    index: usize,
    has_wheel: bool,
}

impl MousePacketParser {
    pub const fn new() -> Self {
        Self {
            buffer: [0; 4],
            index: 0,
            has_wheel: false,
        }
    }

    pub fn set_has_wheel(&mut self, has_wheel: bool) {
        self.has_wheel = has_wheel;
    }

    pub fn reset(&mut self) {
        self.index = 0;
    }

    /// Feed a raw byte from the 8042 auxiliary port into the parser.
    /// Returns `Some(MousePacket)` when a complete, valid packet is framed.
    pub fn process_byte(&mut self, byte: u8) -> Option<MousePacket> {
        // Byte 0 must have bit 3 set to 1. If not, we are out of sync!
        if self.index == 0 && (byte & 0x08) == 0 {
            // Discard out-of-sync byte
            return None;
        }

        self.buffer[self.index] = byte;
        self.index += 1;

        let packet_len = if self.has_wheel { 4 } else { 3 };
        if self.index < packet_len {
            return None;
        }

        // We have a full packet. Reset index for next packet.
        self.index = 0;

        let flags = self.buffer[0];
        let raw_x = self.buffer[1];
        let raw_y = self.buffer[2];

        // Check for overflow bits (bits 6 and 7). If overflowed, discard or clamp.
        let x_overflow = (flags & 0x40) != 0;
        let y_overflow = (flags & 0x80) != 0;
        if x_overflow || y_overflow {
            return None;
        }

        // 9-bit signed sign extension:
        // If bit 4 is set, dx is negative.
        let mut dx = raw_x as i16;
        if (flags & 0x10) != 0 {
            dx |= !0xFF;
        }

        // If bit 5 is set, dy is negative.
        let mut dy = raw_y as i16;
        if (flags & 0x20) != 0 {
            dy |= !0xFF;
        }

        // Scroll wheel (Z delta) from 4th byte: signed 4-bit or 8-bit
        let dz = if self.has_wheel {
            let z_raw = self.buffer[3];
            // Sign extend lower 4 bits if standard IntelliMouse
            if (z_raw & 0x08) != 0 {
                (z_raw | 0xF0) as i8
            } else {
                (z_raw & 0x0F) as i8
            }
        } else {
            0
        };

        Some(MousePacket {
            left_button: (flags & 0x01) != 0,
            right_button: (flags & 0x02) != 0,
            middle_button: (flags & 0x04) != 0,
            dx,
            dy,
            dz,
        })
    }
}
