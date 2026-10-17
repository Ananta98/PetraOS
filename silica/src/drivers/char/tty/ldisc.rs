// SPDX-License-Identifier: GPL-2.0

//! TTY Line Discipline.
//!
//! Based on Zinnia TTY line discipline and POSIX termios specifications.
//! Handles input buffering, CR/NL translation, canonical editing,
//! signal generation, and output post-processing.

use alloc::{collections::VecDeque, vec::Vec};

use crate::api::signal::Signal;
use crate::api::termios::{
    Termios, ECHO, ECHOE, ECHOK, ICANON, ICRNL, IGNCR, INLCR, ISIG, ISTRIP, NOFLSH, ONLCR, OPOST,
    VEOF, VEOL, VERASE, VINTR, VKILL, VQUIT, VSUSP, VWERASE,
};
use super::Tty;

pub const CANON_BUF_SIZE: usize = 4096;

/// Line discipline state machine and buffers.
#[derive(Debug)]
pub struct LineDiscipline {
    pub termios: Termios,
    pub read_buf: VecDeque<u8>,
    pub canon_buf: Vec<u8>,
    pub eof_received: bool,
}

impl LineDiscipline {
    pub fn new() -> Self {
        Self {
            termios: Termios::default(),
            read_buf: VecDeque::with_capacity(CANON_BUF_SIZE),
            canon_buf: Vec::with_capacity(128),
            eof_received: false,
        }
    }

    pub fn is_canon(&self) -> bool {
        self.termios.c_lflag & ICANON != 0
    }

    pub fn is_echo(&self) -> bool {
        self.termios.c_lflag & ECHO != 0
    }

    pub fn is_echoe(&self) -> bool {
        self.termios.c_lflag & ECHOE != 0
    }

    pub fn is_echok(&self) -> bool {
        self.termios.c_lflag & ECHOK != 0
    }

    pub fn is_isig(&self) -> bool {
        self.termios.c_lflag & ISIG != 0
    }

    pub fn is_icrnl(&self) -> bool {
        self.termios.c_iflag & ICRNL != 0
    }

    pub fn is_igncr(&self) -> bool {
        self.termios.c_iflag & IGNCR != 0
    }

    pub fn is_inlcr(&self) -> bool {
        self.termios.c_iflag & INLCR != 0
    }

    pub fn is_istrip(&self) -> bool {
        self.termios.c_iflag & ISTRIP != 0
    }

    pub fn is_opost(&self) -> bool {
        self.termios.c_oflag & OPOST != 0
    }

    pub fn is_onlcr(&self) -> bool {
        self.termios.c_oflag & ONLCR != 0
    }

    pub fn cc(&self, idx: usize) -> u8 {
        self.termios.c_cc[idx]
    }

    pub fn clear(&mut self) {
        self.read_buf.clear();
        self.canon_buf.clear();
        self.eof_received = false;
    }

    /// Appends output bytes with post-processing (e.g. \n -> \r\n if OPOST & ONLCR).
    pub fn append_output(&self, data: &[u8], out: &mut Vec<u8>) {
        if !self.is_opost() {
            out.extend_from_slice(data);
            return;
        }

        for &byte in data {
            if byte == b'\n' && self.is_onlcr() {
                out.extend_from_slice(b"\r\n");
            } else {
                out.push(byte);
            }
        }
    }

    /// Translates output data buffer for writing to hardware driver.
    pub fn write_output(&self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        self.append_output(data, &mut out);
        out
    }

    /// Feeds an incoming character into the line discipline.
    /// Returns any echo bytes that should be output to the terminal.
    pub fn input_char(&mut self, mut byte: u8, tty: &Tty) -> Vec<u8> {
        let mut output = Vec::new();

        if self.is_istrip() {
            byte &= 0x7F;
        }

        // CR / NL translation
        if byte == b'\r' {
            if self.is_igncr() {
                return output;
            }
            if self.is_icrnl() {
                byte = b'\n';
            }
        } else if byte == b'\n' && self.is_inlcr() {
            byte = b'\r';
        }

        // Signal generation (ISIG)
        if self.is_isig() {
            if byte == self.cc(VINTR) {
                tty.signal_foreground(Signal::SIGINT);
                if (self.termios.c_lflag & NOFLSH) == 0 {
                    self.clear();
                }
                if self.is_echo() {
                    self.append_output(b"^C\n", &mut output);
                }
                return output;
            }
            if byte == self.cc(VQUIT) {
                tty.signal_foreground(Signal::SIGQUIT);
                if (self.termios.c_lflag & NOFLSH) == 0 {
                    self.clear();
                }
                if self.is_echo() {
                    self.append_output(b"^\\\n", &mut output);
                }
                return output;
            }
            if byte == self.cc(VSUSP) {
                tty.signal_foreground(Signal::SIGTSTP);
                if self.is_echo() {
                    self.append_output(b"^Z\n", &mut output);
                }
                return output;
            }
        }

        if self.is_canon() {
            self.input_canon(byte, tty, &mut output);
        } else {
            self.input_raw(byte, tty, &mut output);
        }

        output
    }

    fn input_canon(&mut self, byte: u8, tty: &Tty, output: &mut Vec<u8>) {
        let verase = self.cc(VERASE);
        let vkill = self.cc(VKILL);
        let veof = self.cc(VEOF);
        let veol = self.cc(VEOL);
        let vwerase = self.cc(VWERASE);

        // Erase character (Backspace / DEL)
        if byte == verase || byte == 0x7F || byte == 0x08 {
            if self.canon_buf.pop().is_some() && self.is_echo() {
                self.append_output(b"\x08 \x08", output);
            }
            return;
        }

        // Word erase (Ctrl+W)
        if vwerase != 0 && byte == vwerase {
            let mut erased = 0;
            while let Some(&last) = self.canon_buf.last() {
                if last == b' ' || last == b'\t' {
                    self.canon_buf.pop();
                    erased += 1;
                } else {
                    break;
                }
            }
            while let Some(&last) = self.canon_buf.last() {
                if last != b' ' && last != b'\t' {
                    self.canon_buf.pop();
                    erased += 1;
                } else {
                    break;
                }
            }
            if self.is_echo() {
                for _ in 0..erased {
                    self.append_output(b"\x08 \x08", output);
                }
            }
            return;
        }

        // Line kill (^U)
        if vkill != 0 && byte == vkill {
            let count = self.canon_buf.len();
            self.canon_buf.clear();
            if self.is_echo() && self.is_echok() {
                for _ in 0..count {
                    self.append_output(b"\x08 \x08", output);
                }
            }
            return;
        }

        // EOF character (^D)
        if veof != 0 && byte == veof {
            if self.canon_buf.is_empty() {
                self.eof_received = true;
            } else {
                for &b in &self.canon_buf {
                    self.read_buf.push_back(b);
                }
                self.canon_buf.clear();
            }
            tty.wake_readers();
            return;
        }

        // Normal character
        if self.canon_buf.len() < CANON_BUF_SIZE {
            self.canon_buf.push(byte);
        }

        if self.is_echo() {
            self.append_output(&[byte], output);
        }

        // End of line
        if byte == b'\n' || (veol != 0 && byte == veol) {
            for &b in &self.canon_buf {
                self.read_buf.push_back(b);
            }
            self.canon_buf.clear();
            tty.wake_readers();
        }
    }

    fn input_raw(&mut self, byte: u8, tty: &Tty, output: &mut Vec<u8>) {
        self.read_buf.push_back(byte);
        if self.is_echo() {
            self.append_output(&[byte], output);
        }
        tty.wake_readers();
    }

    pub fn read_available(&self) -> usize {
        self.read_buf.len()
    }

    pub fn read_into(&mut self, buf: &mut [u8]) -> usize {
        let mut count = 0;
        let is_canon = self.is_canon();
        let veol = self.cc(VEOL);

        for slot in buf.iter_mut() {
            if let Some(byte) = self.read_buf.pop_front() {
                *slot = byte;
                count += 1;
                if is_canon && (byte == b'\n' || (veol != 0 && byte == veol)) {
                    break;
                }
            } else {
                break;
            }
        }
        count
    }
}

impl Default for LineDiscipline {
    fn default() -> Self {
        Self::new()
    }
}
