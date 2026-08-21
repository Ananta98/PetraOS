//! POSIX `termios` & Terminal Line Discipline
//!
//! Provides structures, bitflags, IOCTL constants, and line discipline
//! management conforming to the Linux x86_64 ABI and POSIX.1-2017 standards.

use crate::ipc::signal;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

pub const NCCS: usize = 32;

// Indices into c_cc array
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

// Input flags (c_iflag)
pub const IGNBRK: u32 = 0o000001;
pub const BRKINT: u32 = 0o000002;
pub const IGNPAR: u32 = 0o000004;
pub const PARMRK: u32 = 0o000010;
pub const INPCK: u32 = 0o000020;
pub const ISTRIP: u32 = 0o000040;
pub const INLCR: u32 = 0o000100;
pub const IGNCR: u32 = 0o000200;
pub const ICRNL: u32 = 0o000400;
pub const IUCLC: u32 = 0o001000;
pub const IXON: u32 = 0o002000;
pub const IXANY: u32 = 0o004000;
pub const IXOFF: u32 = 0o010000;
pub const IMAXBEL: u32 = 0o020000;
pub const IUTF8: u32 = 0o040000;

// Output flags (c_oflag)
pub const OPOST: u32 = 0o000001;
pub const OLCUC: u32 = 0o000002;
pub const ONLCR: u32 = 0o000004;
pub const OCRNL: u32 = 0o000010;
pub const ONOCR: u32 = 0o000020;
pub const ONLRET: u32 = 0o000040;
pub const OFILL: u32 = 0o000100;
pub const OFDEL: u32 = 0o000200;

// Control flags (c_cflag)
pub const CSIZE: u32 = 0o000060;
pub const CS5: u32 = 0o000000;
pub const CS6: u32 = 0o000020;
pub const CS7: u32 = 0o000040;
pub const CS8: u32 = 0o000060;
pub const CSTOPB: u32 = 0o000100;
pub const CREAD: u32 = 0o000200;
pub const PARENB: u32 = 0o000400;
pub const PARODD: u32 = 0o001000;
pub const HUPCL: u32 = 0o002000;
pub const CLOCAL: u32 = 0o004000;

// Local flags (c_lflag)
pub const ISIG: u32 = 0o000001;
pub const ICANON: u32 = 0o000002;
pub const XCASE: u32 = 0o000004;
pub const ECHO: u32 = 0o000010;
pub const ECHOE: u32 = 0o000020;
pub const ECHOK: u32 = 0o000040;
pub const ECHONL: u32 = 0o000100;
pub const NOFLSH: u32 = 0o000200;
pub const TOSTOP: u32 = 0o000400;
pub const ECHOCTL: u32 = 0o001000;
pub const ECHOPRT: u32 = 0o002000;
pub const ECHOKE: u32 = 0o004000;
pub const FLUSHO: u32 = 0o010000;
pub const PENDIN: u32 = 0o040000;
pub const IEXTEN: u32 = 0o100000;

// IOCTL request codes
pub const TCGETS: u64 = 0x5401;
pub const TCSETS: u64 = 0x5402;
pub const TCSETSW: u64 = 0x5403;
pub const TCSETSF: u64 = 0x5404;
pub const TCGETA: u64 = 0x5405;
pub const TCSETA: u64 = 0x5406;
pub const TCSETAW: u64 = 0x5407;
pub const TCSETAF: u64 = 0x5408;
pub const TCSBRK: u64 = 0x5409;
pub const TCXONC: u64 = 0x540A;
pub const TCFLSH: u64 = 0x540B;
pub const TIOCEXCL: u64 = 0x540C;
pub const TIOCNXCL: u64 = 0x540D;
pub const TIOCSCTTY: u64 = 0x540E;
pub const TIOCGPGRP: u64 = 0x540F;
pub const TIOCSPGRP: u64 = 0x5410;
pub const TIOCOUTQ: u64 = 0x5411;
pub const TIOCSTI: u64 = 0x5412;
pub const TIOCGWINSZ: u64 = 0x5413;
pub const TIOCSWINSZ: u64 = 0x5414;
pub const TIOCMGET: u64 = 0x5415;
pub const TIOCMBIS: u64 = 0x5416;
pub const TIOCMBIC: u64 = 0x5417;
pub const TIOCMSET: u64 = 0x5418;
pub const TIOCINQ: u64 = 0x541B;
pub const FIONREAD: u64 = 0x541B;
pub const TIOCNOTTY: u64 = 0x5422;
pub const TIOCGSID: u64 = 0x5429;
pub const TIOCGPTN: u64 = 0x80045430;
pub const TIOCSPTLCK: u64 = 0x40045431;
pub const TIOCGDEV: u64 = 0x80045432;
pub const TCGETS2: u64 = 0x802C542A;
pub const TCSETS2: u64 = 0x402C542B;
pub const TCSETSW2: u64 = 0x402C542C;
pub const TCSETSF2: u64 = 0x402C542D;

// Flow control actions (TCXONC)
pub const TCOOFF: usize = 0;
pub const TCOON: usize = 1;
pub const TCIOFF: usize = 2;
pub const TCION: usize = 3;

// Queue selector constants (TCFLSH)
pub const TCIFLUSH: usize = 0;
pub const TCOFLUSH: usize = 1;
pub const TCIOFLUSH: usize = 2;

/// Standard POSIX `termios` structure conforming to Linux x86_64 ABI.
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
        let mut cc = [0u8; NCCS];
        cc[VINTR] = 0x03; // Ctrl+C (ETX)
        cc[VQUIT] = 0x1C; // Ctrl+\ (FS)
        cc[VERASE] = 0x7F; // DEL / Backspace
        cc[VKILL] = 0x15; // Ctrl+U (NAK)
        cc[VEOF] = 0x04; // Ctrl+D (EOT)
        cc[VTIME] = 0;
        cc[VMIN] = 1;
        cc[VSWTC] = 0;
        cc[VSTART] = 0x11; // Ctrl+Q (DC1)
        cc[VSTOP] = 0x13; // Ctrl+S (DC3)
        cc[VSUSP] = 0x1A; // Ctrl+Z (SUB)
        cc[VEOL] = 0;
        cc[VREPRINT] = 0x12; // Ctrl+R (DC2)
        cc[VDISCARD] = 0x0F; // Ctrl+O (SI)
        cc[VWERASE] = 0x17; // Ctrl+W (ETB)
        cc[VLNEXT] = 0x16; // Ctrl+V (SYN)
        cc[VEOL2] = 0;

        Self {
            c_iflag: ICRNL | IXON | IUTF8,
            c_oflag: OPOST | ONLCR,
            c_cflag: CS8 | CREAD | HUPCL,
            c_lflag: ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHONL | IEXTEN,
            c_line: 0,
            c_cc: cc,
            c_ispeed: 38400,
            c_ospeed: 38400,
        }
    }
}



/// POSIX terminal window size structure.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinSize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

impl Default for WinSize {
    fn default() -> Self {
        Self {
            ws_row: 25,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }
}

/// Terminal line discipline state machine.
pub struct LineDiscipline {
    pub termios: Termios,
    pub winsize: WinSize,
    pub foreground_pgid: i32,
    canon_buffer: Vec<u8>,
    read_queue: VecDeque<u8>,
}

impl LineDiscipline {
    pub fn new(winsize: WinSize) -> Self {
        Self {
            termios: Termios::default(),
            winsize,
            foreground_pgid: 1,
            canon_buffer: Vec::with_capacity(256),
            read_queue: VecDeque::with_capacity(1024),
        }
    }

    /// Process incoming character from hardware/master device.
    /// Returns any bytes that need to be echoed to the screen/output.
    pub fn accept_input_byte(&mut self, byte: u8) -> Vec<u8> {
        let mut echo_bytes = Vec::new();

        // 1. Input preprocessing (c_iflag)
        let mut processed_byte = byte;
        if (self.termios.c_iflag & ISTRIP) != 0 {
            processed_byte &= 0x7F;
        }
        if processed_byte == b'\r' {
            if (self.termios.c_iflag & IGNCR) != 0 {
                return echo_bytes;
            }
            if (self.termios.c_iflag & ICRNL) != 0 {
                processed_byte = b'\n';
            }
        } else if processed_byte == b'\n' && (self.termios.c_iflag & INLCR) != 0 {
            processed_byte = b'\r';
        }

        // 2. Signal generation (c_lflag & ISIG)
        if (self.termios.c_lflag & ISIG) != 0 {
            if processed_byte == self.termios.c_cc[VINTR] {
                self.send_signal_to_fg(crate::ipc::signal::SIGINT);
                if (self.termios.c_lflag & NOFLSH) == 0 {
                    self.canon_buffer.clear();
                    self.read_queue.clear();
                }
                if (self.termios.c_lflag & ECHO) != 0 {
                    echo_bytes.extend_from_slice(b"^C\n");
                }
                return echo_bytes;
            }
            if processed_byte == self.termios.c_cc[VQUIT] {
                self.send_signal_to_fg(crate::ipc::signal::SIGQUIT);
                if (self.termios.c_lflag & NOFLSH) == 0 {
                    self.canon_buffer.clear();
                    self.read_queue.clear();
                }
                if (self.termios.c_lflag & ECHO) != 0 {
                    echo_bytes.extend_from_slice(b"^\\\n");
                }
                return echo_bytes;
            }
            if processed_byte == self.termios.c_cc[VSUSP] {
                self.send_signal_to_fg(crate::ipc::signal::SIGTSTP);
                if (self.termios.c_lflag & ECHO) != 0 {
                    echo_bytes.extend_from_slice(b"^Z\n");
                }
                return echo_bytes;
            }
        }

        // 3. Canonical vs Raw mode processing
        if (self.termios.c_lflag & ICANON) != 0 {
            // Canonical Mode (Line-buffered)
            self.input_canon(processed_byte, &mut echo_bytes);
        } else {
            // Raw / Non-canonical Mode (Instant availability)
            self.input_raw(processed_byte, &mut echo_bytes);
        }

        echo_bytes
    }

    pub fn input_canon(&mut self, byte: u8, echo_bytes: &mut Vec<u8>) {
        let processed_byte = byte;
        if processed_byte == self.termios.c_cc[VERASE] || processed_byte == 0x08 {
            if let Some(removed) = self.canon_buffer.pop() {
                if (self.termios.c_lflag & ECHO) != 0 && (self.termios.c_lflag & ECHOE) != 0 {
                    // Visual erase: backspace, space, backspace
                    echo_bytes.extend_from_slice(b"\x08 \x08");
                    if removed < 0x20 && (self.termios.c_lflag & ECHOCTL) != 0 {
                        echo_bytes.extend_from_slice(b"\x08 \x08");
                    }
                }
            }
        } else if processed_byte == self.termios.c_cc[VKILL] {
            while let Some(removed) = self.canon_buffer.pop() {
                if (self.termios.c_lflag & ECHO) != 0 && (self.termios.c_lflag & ECHOK) != 0 {
                    echo_bytes.extend_from_slice(b"\x08 \x08");
                    if removed < 0x20 && (self.termios.c_lflag & ECHOCTL) != 0 {
                        echo_bytes.extend_from_slice(b"\x08 \x08");
                    }
                }
            }
        } else if processed_byte == self.termios.c_cc[VEOF] {
            // Flush line buffer to read queue without including the EOF byte
            for b in self.canon_buffer.drain(..) {
                self.read_queue.push_back(b);
            }
        } else if processed_byte == b'\n' || processed_byte == self.termios.c_cc[VEOL] {
            self.canon_buffer.push(processed_byte);
            if (self.termios.c_lflag & ECHO) != 0 || (self.termios.c_lflag & ECHONL) != 0 {
                if (self.termios.c_oflag & OPOST) != 0 && (self.termios.c_oflag & ONLCR) != 0 {
                    echo_bytes.extend_from_slice(b"\r\n");
                } else {
                    echo_bytes.push(b'\n');
                }
            }
            for b in self.canon_buffer.drain(..) {
                self.read_queue.push_back(b);
            }
        } else {
            self.canon_buffer.push(processed_byte);
            if (self.termios.c_lflag & ECHO) != 0 {
                if processed_byte < 0x20 && (self.termios.c_lflag & ECHOCTL) != 0 {
                    echo_bytes.push(b'^');
                    echo_bytes.push(processed_byte + b'@');
                } else {
                    echo_bytes.push(processed_byte);
                }
            }
        }
    }

    pub fn input_raw(&mut self, byte: u8, echo_bytes: &mut Vec<u8>) {
        self.read_queue.push_back(byte);
        if (self.termios.c_lflag & ECHO) != 0 {
            echo_bytes.push(byte);
        }
    }

    /// Read available bytes into `buf`. Returns count read.
    pub fn read_bytes(&mut self, buf: &mut [u8]) -> usize {
        let mut count = 0;
        while count < buf.len() {
            if let Some(byte) = self.read_queue.pop_front() {
                buf[count] = byte;
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Check if bytes are ready for reading.
    pub fn available_read_bytes(&self) -> usize {
        self.read_queue.len()
    }

    /// Flush input/output queues based on POSIX queue selector:
    /// TCIFLUSH (0), TCOFLUSH (1), TCIOFLUSH (2).
    pub fn flush_queue(&mut self, queue_selector: usize) {
        match queue_selector {
            0 => {
                self.canon_buffer.clear();
                self.read_queue.clear();
            }
            1 => {
                // TCOFLUSH: Output queue flush (line discipline streams directly)
            }
            2 => {
                self.canon_buffer.clear();
                self.read_queue.clear();
            }
            _ => {}
        }
    }

    /// Process output bytes (c_oflag translation).
    pub fn process_output_bytes(&self, input: &[u8]) -> Vec<u8> {
        if (self.termios.c_oflag & OPOST) == 0 {
            return input.to_vec();
        }

        let mut out = Vec::with_capacity(input.len() + 16);
        for &b in input {
            if b == b'\n' && (self.termios.c_oflag & ONLCR) != 0 {
                out.push(b'\r');
                out.push(b'\n');
            } else if b == b'\r' && (self.termios.c_oflag & OCRNL) != 0 {
                out.push(b'\n');
            } else {
                out.push(b);
            }
        }
        out
    }

    /// Send signal to the foreground process group.
    fn send_signal_to_fg(&self, signum: u8) {
        if self.foreground_pgid > 0 {
            let _ = signal::send_signal_to_process_group(self.foreground_pgid, signum);
        }
    }
}
