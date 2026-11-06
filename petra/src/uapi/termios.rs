// SPDX-License-Identifier: GPL-2.0

#![allow(non_camel_case_types)]

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub type tcflag_t = u32;
pub type cc_t = u8;

/// Control characters in the kernel `struct termios` (`TCGETS`/`TCSETS`).
///
/// libc's `struct termios` is larger (glibc: 32 characters plus two speed
/// fields, 60 bytes); glibc converts to this 36-byte layout before the ioctl, so
/// writing anything bigger overruns the caller's stack.
pub const NCCS: usize = 19;

#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct termios {
    pub c_iflag: tcflag_t,
    pub c_oflag: tcflag_t,
    pub c_cflag: tcflag_t,
    pub c_lflag: tcflag_t,
    pub c_line: cc_t,
    pub c_cc: [cc_t; NCCS],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

impl termios {
    pub const SIZE: usize = 36;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        zerocopy::transmute!(*self)
    }

    pub fn from_bytes(buf: &[u8; Self::SIZE]) -> Self {
        zerocopy::transmute!(*buf)
    }
}

impl winsize {
    pub const SIZE: usize = 8;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        zerocopy::transmute!(*self)
    }

    pub fn from_bytes(buf: &[u8; Self::SIZE]) -> Self {
        zerocopy::transmute!(*buf)
    }
}

// c_lflag bits
pub const ISIG: tcflag_t = 0o000001;
pub const ICANON: tcflag_t = 0o000002;
pub const ECHO: tcflag_t = 0o000010;
pub const ECHOE: tcflag_t = 0o000020;
pub const ECHOK: tcflag_t = 0o000040;
pub const ECHONL: tcflag_t = 0o000100;
pub const NOFLSH: tcflag_t = 0o000200;
pub const TOSTOP: tcflag_t = 0o000400;
pub const IEXTEN: tcflag_t = 0o100000;

// c_cc indices
pub const VINTR: usize = 0;
pub const VQUIT: usize = 1;
pub const VERASE: usize = 2;
pub const VKILL: usize = 3;
pub const VEOF: usize = 4;
pub const VTIME: usize = 5;
pub const VMIN: usize = 6;
pub const VSWTC: usize = 7;
pub const VSTART: usize = 8;
pub const VSTOP: usize = 9;
pub const VSUSP: usize = 10;
pub const VEOL: usize = 11;
pub const VREPRINT: usize = 12;
pub const VDISCARD: usize = 13;
pub const VWERASE: usize = 14;
pub const VLNEXT: usize = 15;
pub const VEOL2: usize = 16;

// c_iflag bits
pub const IGNBRK: tcflag_t = 0o000001;
pub const BRKINT: tcflag_t = 0o000002;
pub const IGNPAR: tcflag_t = 0o000004;
pub const PARMRK: tcflag_t = 0o000010;
pub const INPCK: tcflag_t = 0o000020;
pub const ISTRIP: tcflag_t = 0o000040;
pub const INLCR: tcflag_t = 0o000100;
pub const IGNCR: tcflag_t = 0o000200;
pub const ICRNL: tcflag_t = 0o000400;
pub const IXON: tcflag_t = 0o002000;
pub const IXANY: tcflag_t = 0o004000;
pub const IXOFF: tcflag_t = 0o010000;

// c_oflag bits
pub const OPOST: tcflag_t = 0o000001;
pub const ONLCR: tcflag_t = 0o000004;
pub const OCRNL: tcflag_t = 0o000010;
pub const ONOCR: tcflag_t = 0o000020;
pub const ONLRET: tcflag_t = 0o000040;

// c_cflag bits
/// The line speed lives in `c_cflag` in the kernel layout.
pub const B38400: tcflag_t = 0o000017;
pub const CSIZE: tcflag_t = 0o000060;
pub const CS5: tcflag_t = 0o000000;
pub const CS6: tcflag_t = 0o000020;
pub const CS7: tcflag_t = 0o000040;
pub const CS8: tcflag_t = 0o000060;
pub const CSTOPB: tcflag_t = 0o000100;
pub const CREAD: tcflag_t = 0o000200;
pub const PARENB: tcflag_t = 0o000400;
pub const PARODD: tcflag_t = 0o001000;
pub const HUPCL: tcflag_t = 0o002000;
pub const CLOCAL: tcflag_t = 0o004000;

pub type Termios = termios;

impl Default for termios {
    fn default() -> Self {
        let mut cc = [0; NCCS];
        cc[VINTR] = 3; // ^C
        cc[VQUIT] = 28; // ^\
        cc[VERASE] = 127; // DEL
        cc[VKILL] = 21; // ^U
        cc[VEOF] = 4; // ^D
        cc[VTIME] = 0;
        cc[VMIN] = 1;
        cc[VSWTC] = 0;
        cc[VSTART] = 17; // ^Q
        cc[VSTOP] = 19; // ^S
        cc[VSUSP] = 26; // ^Z
        cc[VEOL] = 0;
        cc[VREPRINT] = 18; // ^R
        cc[VDISCARD] = 15; // ^O
        cc[VWERASE] = 23; // ^W (Ctrl+W)
        cc[VLNEXT] = 22; // ^V
        cc[VEOL2] = 0;

        Self {
            c_iflag: ICRNL | IXON,
            c_oflag: OPOST | ONLCR,
            c_cflag: B38400 | CS8 | CREAD | HUPCL,
            c_lflag: ISIG | ICANON | ECHO | ECHOE | ECHOK,
            c_line: 0,
            c_cc: cc,
        }
    }
}
