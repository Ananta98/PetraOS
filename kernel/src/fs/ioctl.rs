// --- ioctl Command Constants ---
pub const TCGETS: usize = 0x5401;
pub const TCSETS: usize = 0x5402;
pub const TCSETSW: usize = 0x5403;
pub const TCSETSF: usize = 0x5404;
pub const TIOCNOTTY: usize = 0x540B;
pub const TIOCSCTTY: usize = 0x540E;
pub const TIOCGPGRP: usize = 0x540F;
pub const TIOCSPGRP: usize = 0x5410;
pub const TIOCGWINSZ: usize = 0x5413;
pub const TIOCSWINSZ: usize = 0x5414;
pub const FIONREAD: usize = 0x541B;
pub const FIONBIO: usize = 0x5421;

// --- Data Structures ---

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

impl Winsize {
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0..2].copy_from_slice(&self.ws_row.to_ne_bytes());
        buf[2..4].copy_from_slice(&self.ws_col.to_ne_bytes());
        buf[4..6].copy_from_slice(&self.ws_xpixel.to_ne_bytes());
        buf[6..8].copy_from_slice(&self.ws_ypixel.to_ne_bytes());
        buf
    }
}

#[repr(C, packed)]
#[derive(Default, Copy, Clone)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; 32],
    pub _padding: [u8; 3], // 3 padding bytes for 60-byte x86_64 alignment
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

impl Termios {
    /// Returns the default Linux x86_64 termios configuration
    pub fn default_linux() -> Self {
        let mut t = Self {
            c_iflag: 0x500,        // ICRNL | IXON
            c_oflag: 0x5,          // OPOST | ONLCR
            c_cflag: 0xbf,         // CS8 | CREAD | HUPCL | B38400
            c_lflag: 0x8a3b,       // ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHOCTL | ECHOKE
            c_ispeed: 0x0000_000f, // B38400
            c_ospeed: 0x0000_000f, // B38400
            ..Default::default()
        };

        // Default control characters
        t.c_cc[0] = 0x03; // VINTR = Ctrl-C
        t.c_cc[1] = 0x1c; // VQUIT
        t.c_cc[2] = 0x7f; // VERASE
        t.c_cc[3] = 0x15; // VKILL
        t.c_cc[4] = 0x04; // VEOF = Ctrl-D
        t.c_cc[5] = 0x00; // VTIME
        t.c_cc[6] = 0x01; // VMIN

        t
    }

    pub fn to_bytes(&self) -> [u8; 60] {
        let mut buf = [0u8; 60];
        buf[0..4].copy_from_slice(&self.c_iflag.to_ne_bytes());
        buf[4..8].copy_from_slice(&self.c_oflag.to_ne_bytes());
        buf[8..12].copy_from_slice(&self.c_cflag.to_ne_bytes());
        buf[12..16].copy_from_slice(&self.c_lflag.to_ne_bytes());
        buf[16] = self.c_line;
        buf[17..49].copy_from_slice(&self.c_cc);
        buf[49..52].copy_from_slice(&self._padding);
        buf[52..56].copy_from_slice(&self.c_ispeed.to_ne_bytes());
        buf[56..60].copy_from_slice(&self.c_ospeed.to_ne_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8; 60]) -> Self {
        let c_iflag = u32::from_ne_bytes(buf[0..4].try_into().unwrap());
        let c_oflag = u32::from_ne_bytes(buf[4..8].try_into().unwrap());
        let c_cflag = u32::from_ne_bytes(buf[8..12].try_into().unwrap());
        let c_lflag = u32::from_ne_bytes(buf[12..16].try_into().unwrap());
        let c_line = buf[16];
        let mut c_cc = [0u8; 32];
        c_cc.copy_from_slice(&buf[17..49]);
        let mut _padding = [0u8; 3];
        _padding.copy_from_slice(&buf[49..52]);
        let c_ispeed = u32::from_ne_bytes(buf[52..56].try_into().unwrap());
        let c_ospeed = u32::from_ne_bytes(buf[56..60].try_into().unwrap());

        Self {
            c_iflag,
            c_oflag,
            c_cflag,
            c_lflag,
            c_line,
            c_cc,
            _padding,
            c_ispeed,
            c_ospeed,
        }
    }
}
