//! POSIX/Linux Termios and Terminal Control Structures
//!
//! Provides the data structures, flags, control characters, and IOCTL constants
//! for terminal line discipline and window size control.

use crate::sync::spinlock::Spinlock;

// Standard Linux Terminal IOCTL Constants
pub const TCGETS: u64 = 0x5401;
pub const TCSETS: u64 = 0x5402;
pub const TCSETSW: u64 = 0x5403;
pub const TCSETSF: u64 = 0x5404;
pub const TCGETA: u64 = 0x5405;
pub const TCSETA: u64 = 0x5406;
pub const TCSETAW: u64 = 0x5407;
pub const TCSETAF: u64 = 0x5408;
pub const TIOCSCTTY: u64 = 0x540E;
pub const TIOCGPGRP: u64 = 0x540F;
pub const TIOCSPGRP: u64 = 0x5410;
pub const TIOCGWINSZ: u64 = 0x5413;
pub const TIOCSWINSZ: u64 = 0x5414;
pub const NCCS: usize = 32;

// Standard Linux termios c_cc indices
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

// Standard Linux termios c_lflag bits
pub const ISIG: u32 = 0x0001;
pub const ICANON: u32 = 0x0002;
pub const ECHO: u32 = 0x0008;
pub const ECHOE: u32 = 0x0010;
pub const ECHOK: u32 = 0x0020;
pub const ECHONL: u32 = 0x0040;
pub const NOFLSH: u32 = 0x0080;
pub const TOSTOP: u32 = 0x0100;
pub const ECHOCTL: u32 = 0x0200;
pub const ECHOPRT: u32 = 0x0400;
pub const ECHOKE: u32 = 0x0800;
pub const FLUSHO: u32 = 0x1000;
pub const PENDIN: u32 = 0x4000;
pub const IEXTEN: u32 = 0x8000;

// Standard Linux termios c_iflag bits
pub const IGNBRK: u32 = 0x0001;
pub const BRKINT: u32 = 0x0002;
pub const IGNPAR: u32 = 0x0004;
pub const PARMRK: u32 = 0x0008;
pub const INPCK: u32 = 0x0010;
pub const ISTRIP: u32 = 0x0020;
pub const INLCR: u32 = 0x0040;
pub const IGNCR: u32 = 0x0080;
pub const ICRNL: u32 = 0x0100;
pub const IXON: u32 = 0x0400;
pub const IXOFF: u32 = 0x1000;

// Standard Linux termios c_oflag bits
pub const OPOST: u32 = 0x0001;
pub const ONLCR: u32 = 0x0004;

/// Global termios settings for the system console.
pub static CONSOLE_TERMIOS: Spinlock<Termios> = Spinlock::new(Termios::default_console());

/// x86_64 Linux winsize structure for window size control.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinSize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

impl WinSize {
    pub const fn new(rows: u16, cols: u16, xpixel: u16, ypixel: u16) -> Self {
        Self {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: xpixel,
            ws_ypixel: ypixel,
        }
    }
}

/// x86_64 Linux termios structure for terminal control (ABI-compatible with mlibc/Linux).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; NCCS],
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

impl Default for Termios {
    fn default() -> Self {
        Self::default_console()
    }
}

impl Termios {
    /// Creates the standard default console configuration.
    pub const fn default_console() -> Self {
        let mut c_cc = [0u8; NCCS];
        c_cc[VINTR] = 3; // ^C
        c_cc[VQUIT] = 28; // ^\
        c_cc[VERASE] = 127; // DEL / Backspace
        c_cc[VKILL] = 21; // ^U
        c_cc[VEOF] = 4; // ^D
        c_cc[VTIME] = 0;
        c_cc[VMIN] = 1;
        c_cc[VSWTC] = 0;
        c_cc[VSTART] = 17; // ^Q
        c_cc[VSTOP] = 19; // ^S
        c_cc[VSUSP] = 26; // ^Z
        c_cc[VEOL] = 0;
        c_cc[VREPRINT] = 18; // ^R
        c_cc[VDISCARD] = 15; // ^O
        c_cc[VWERASE] = 23; // ^W
        c_cc[VLNEXT] = 22; // ^V
        c_cc[VEOL2] = 0;

        Self {
            c_iflag: ICRNL | IXON,
            c_oflag: OPOST | ONLCR,
            c_cflag: 0x00bf, // B38400 | CS8 | CREAD | HUPCL
            c_lflag: ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHOCTL | ECHOKE,
            c_line: 0,
            c_cc,
            c_ispeed: 38400,
            c_ospeed: 38400,
        }
    }

    /// Check if canonical input processing is active.
    #[inline]
    pub const fn is_canonical(&self) -> bool {
        (self.c_lflag & ICANON) != 0
    }

    /// Check if character echoing is enabled.
    #[inline]
    pub const fn is_echo(&self) -> bool {
        (self.c_lflag & ECHO) != 0
    }

    /// Check if signal generation is enabled.
    #[inline]
    pub const fn is_sig_enabled(&self) -> bool {
        (self.c_lflag & ISIG) != 0
    }
}
