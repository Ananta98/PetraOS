// SPDX-License-Identifier: GPL-2.0

//! System Console Driver
//!
//! Provides the primary system console (`/dev/console`).
//! Integrates the line discipline and delegates to the framebuffer console.

use alloc::string::String;

use crate::api::errno::Result;
use crate::drivers::char::tty::ldisc::LineDiscipline;
use crate::drivers::CharacterDevice;

/// Helper to iterate through output buffer chunks, splitting by `\n` if `opost && onlcr` is active.
fn for_each_line_chunk(buf: &[u8], opost_onlcr: bool, mut on_chunk: impl FnMut(&[u8])) {
    if !opost_onlcr {
        on_chunk(buf);
        return;
    }
    let mut start = 0;
    while let Some(pos) = buf[start..].iter().position(|&b| b == b'\n') {
        let nl_idx = start + pos;
        if nl_idx > start {
            on_chunk(&buf[start..nl_idx]);
        }
        on_chunk(b"\r\n");
        start = nl_idx + 1;
    }
    if start < buf.len() {
        on_chunk(&buf[start..]);
    }
}

/// The main system console device
pub struct Console {
    ldisc: LineDiscipline,
}

impl Console {
    pub fn new() -> Self {
        Self {
            ldisc: LineDiscipline::new(),
        }
    }

    /// Process an incoming character from the keyboard into the line discipline.
    pub fn handle_input(&self, ch: u8) {
        self.ldisc.receive_char(ch, &mut |echo_slice| {
            if let Some(cons_lock) = crate::drivers::drm::get_console() {
                let mut cons = cons_lock.lock();
                cons.write_bytes(echo_slice);
            }
        });
    }

    /// Wakes readers blocked on the line discipline and pollers (called from IRQ context).
    pub fn notify_input(&self) {
        self.ldisc.wake_readers();
    }

    /// Reports poll readiness for reading/writing.
    pub fn poll(&self) -> Result<u32> {
        super::drain_input();
        self.ldisc.poll()
    }
}

impl Default for Console {
    fn default() -> Self {
        Self::new()
    }
}

impl CharacterDevice for Console {
    fn name(&self) -> String {
        String::from("console")
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        super::drain_input();
        self.ldisc.read(buf)
    }

    fn write(&self, buf: &[u8]) -> Result<usize> {
        let (opost, onlcr) = self.ldisc.output_flags();

        // Write batch directly to framebuffer text console
        if let Some(cons_lock) = crate::drivers::drm::get_console() {
            let mut cons = cons_lock.lock();
            for_each_line_chunk(buf, opost && onlcr, |chunk| {
                cons.write_bytes(chunk);
            });
        }

        Ok(buf.len())
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        self.ldisc.ioctl(cmd, arg)
    }

    fn poll(&self) -> Result<u32> {
        self.poll()
    }
}
