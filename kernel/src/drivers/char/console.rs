use super::{CharDevice, InputBuffer};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use ostd::sync::SpinLock;
use spin::Once;

/// A character device implementing `/dev/console`.
///
/// # Input (read side)
///
/// Bytes enter the console via [`ConsoleDriver::push_input`] (called by the
/// keyboard ISR or other input source).  Two modes are supported:
///
/// **Canonical mode** (default) – the driver performs line-editing:
/// - Backspace (`0x08` / `0x7f`) erases the previous character.
/// - Enter (`\r` / `\n`) finalises the current line and makes it available
///   to readers.
/// - Echo is enabled by default, so typed characters are immediately written
///   back to the output.
///
/// **Raw mode** – every incoming byte is directly queued for readers without
/// any processing.
///
/// # Output (write side)
///
/// Written bytes are forwarded to the host serial port.  A lone `\n` (LF) is
/// automatically expanded to `\r\n` so that serial terminals display a correct
/// newline.
pub struct ConsoleDriver {
    raw_input: InputBuffer,

    /// Mode flags.
    echo: AtomicBool,
    canonical: AtomicBool,

    /// Canonical-mode state.
    partial_line: SpinLock<Vec<u8>>,
    completed_lines: SpinLock<VecDeque<Vec<u8>>>,
}

impl ConsoleDriver {
    pub fn new() -> Self {
        Self {
            raw_input: InputBuffer::new(4096),
            echo: AtomicBool::new(true),
            canonical: AtomicBool::new(true),
            partial_line: SpinLock::new(Vec::new()),
            completed_lines: SpinLock::new(VecDeque::new()),
        }
    }

    // ------------------------------------------------------------------
    // Public control knobs
    // ------------------------------------------------------------------

    pub fn set_echo(&self, enabled: bool) {
        self.echo.store(enabled, Ordering::Relaxed);
    }

    pub fn is_echo(&self) -> bool {
        self.echo.load(Ordering::Relaxed)
    }

    pub fn set_canonical(&self, enabled: bool) {
        self.canonical.store(enabled, Ordering::Relaxed);
    }

    pub fn is_canonical(&self) -> bool {
        self.canonical.load(Ordering::Relaxed)
    }

    // ------------------------------------------------------------------
    // Feed bytes from the keyboard ISR (or other input source)
    // ------------------------------------------------------------------

    /// Enqueue raw bytes from an input source.
    ///
    /// In canonical mode each byte is immediately processed for line editing
    /// and echo; in raw mode bytes are simply buffered.
    pub fn push_input(&self, data: &[u8]) {
        if self.canonical.load(Ordering::Relaxed) {
            for &byte in data {
                self.process_canonical(byte);
            }
        } else {
            self.raw_input.push(data);
        }
    }

    /// Process a single byte in canonical mode.
    fn process_canonical(&self, byte: u8) {
        match byte {
            b'\r' | b'\n' => self.finish_line(),
            0x08 | 0x7f => self.handle_backspace(),
            0x03 => {
                // Ctrl-C – discard the current line.
                self.partial_line.lock().clear();
                if self.echo.load(Ordering::Relaxed) {
                    self.echo_str("^C\r\n");
                }
            }
            _ => self.insert_char(byte),
        }
    }

    fn finish_line(&self) {
        let mut partial = self.partial_line.lock();
        if self.echo.load(Ordering::Relaxed) {
            self.echo_str("\r\n");
        }
        let line = core::mem::take(&mut *partial);
        self.completed_lines.lock().push_back(line);
    }

    fn handle_backspace(&self) {
        let mut partial = self.partial_line.lock();
        if !partial.is_empty() {
            partial.pop();
            if self.echo.load(Ordering::Relaxed) {
                self.echo_str("\x08 \x08");
            }
        }
    }

    fn insert_char(&self, byte: u8) {
        let mut partial = self.partial_line.lock();
        partial.push(byte);
        if self.echo.load(Ordering::Relaxed) {
            let buf = [byte];
            self.echo_slice(&buf);
        }
    }

    // ------------------------------------------------------------------
    // Output helpers
    // ------------------------------------------------------------------

    /// Write bytes to the serial port (used for echoing as well).
    fn serial_write(&self, buf: &[u8]) {
        if let Ok(s) = core::str::from_utf8(buf) {
            ostd::console::early_print(format_args!("{}", s));
        }
    }

    /// Echo a &str (avoids the UTF-8 check since it's already valid).
    fn echo_str(&self, s: &str) {
        ostd::console::early_print(format_args!("{}", s));
    }

    /// Echo a byte slice.
    fn echo_slice(&self, buf: &[u8]) {
        self.serial_write(buf);
    }

    /// Flush a chunk of processed output bytes.
    fn flush_output(&self, buf: &[u8]) {
        self.serial_write(buf);
    }
}

// -----------------------------------------------------------------------
// CharDevice trait implementation
// -----------------------------------------------------------------------

impl CharDevice for ConsoleDriver {
    /// Read from the console.
    ///
    /// In **canonical** mode this returns one completed line at a time (the
    /// line terminator is stripped).  When no line is ready the call returns
    /// `Ok(0)` (non-blocking).
    ///
    /// In **raw** mode buffered bytes are drained directly.
    fn read(&self, buf: &mut [u8]) -> Result<usize, ostd::Error> {
        if self.canonical.load(Ordering::Relaxed) {
            let mut lines = self.completed_lines.lock();
            if let Some(line) = lines.pop_front() {
                let n = core::cmp::min(buf.len(), line.len());
                buf[..n].copy_from_slice(&line[..n]);
                Ok(n)
            } else {
                Ok(0)
            }
        } else {
            self.raw_input.read_into(buf)
        }
    }

    /// Write to the console (serial port).
    ///
    /// A bare `\n` is expanded to `\r\n` so serial terminals display
    /// correctly.
    fn write(&self, buf: &[u8]) -> Result<usize, ostd::Error> {
        // Process the buffer in chunks, converting \n → \r\n.
        const CHUNK: usize = 256;
        let mut tmp = [0u8; CHUNK];
        let mut pos = 0usize;

        for &byte in buf {
            if byte == b'\n' {
                if pos + 2 > tmp.len() {
                    self.flush_output(&tmp[..pos]);
                    pos = 0;
                }
                tmp[pos] = b'\r';
                tmp[pos + 1] = b'\n';
                pos += 2;
            } else {
                if pos + 1 > tmp.len() {
                    self.flush_output(&tmp[..pos]);
                    pos = 0;
                }
                tmp[pos] = byte;
                pos += 1;
            }
        }
        if pos > 0 {
            self.flush_output(&tmp[..pos]);
        }
        Ok(buf.len())
    }
}

// -----------------------------------------------------------------------
// Singleton
// -----------------------------------------------------------------------

static CONSOLE: Once<Arc<ConsoleDriver>> = Once::new();

/// Register the console driver with devfs and store the global singleton.
///
/// Safe to call before `fs::init()` – devfs uses a lazy root inode, so the
/// device node merely becomes visible once `/dev` is mounted.
pub fn init() {
    let driver = Arc::new(ConsoleDriver::new());
    let _ = super::register_char_device("console", driver.clone());
    CONSOLE.call_once(|| driver);
}

/// Return a reference to the global console driver (used by keyboard ISRs to
/// push input bytes).
pub(crate) fn console() -> Option<Arc<ConsoleDriver>> {
    CONSOLE.get().cloned()
}
