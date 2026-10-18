// SPDX-License-Identifier: MPL-2.0

//! Framebuffer Terminal Console powered by `os-terminal`.
//!
//! Provides a standards-compliant VT100 / XTerm terminal emulator
//! rendering directly to the physical linear framebuffer.

use alloc::{boxed::Box, sync::Arc};
use os_terminal::{DrawTarget, Rgb, Terminal};
use spin::Mutex;

use super::font::ConsoleFont;

use crate::{
    api::termios::winsize,
    drivers::drm::framebuffer::{get_framebuffer, FrameBuffer, PixelFormat},
};

/// Hardware display target integrating `FrameBuffer` with `os-terminal`'s `DrawTarget`.
pub struct FramebufferDisplay {
    fb: Arc<FrameBuffer>,
    width: usize,
    height: usize,
    line_size: usize,
    pixel_format: PixelFormat,
    bpp: usize,
}

impl FramebufferDisplay {
    pub fn new(fb: Arc<FrameBuffer>) -> Self {
        let width = fb.width;
        let height = fb.height;
        let line_size = fb.line_size;
        let pixel_format = fb.pixel_format;
        let bpp = pixel_format.nbytes();
        Self {
            fb,
            width,
            height,
            line_size,
            pixel_format,
            bpp,
        }
    }
}

impl DrawTarget for FramebufferDisplay {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    #[inline(always)]
    fn draw_pixel(&mut self, x: usize, y: usize, rgb: Rgb) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = x * self.bpp + y * self.line_size;
        match self.pixel_format {
            PixelFormat::BgrReserved => {
                let pixel = (rgb.2 as u32) | ((rgb.1 as u32) << 8) | ((rgb.0 as u32) << 16);
                let _ = self.fb.write_pixel_u32(offset, pixel);
            }
            PixelFormat::Rgb888 => {
                let _ = self.fb.write_bytes_at(offset, &[rgb.0, rgb.1, rgb.2]);
            }
            PixelFormat::Rgb565 => {
                let r = (rgb.0 >> 3) as u16;
                let g = (rgb.1 >> 2) as u16;
                let b = (rgb.2 >> 3) as u16;
                let rgb565 = (r << 11) | (g << 5) | b;
                let _ = self.fb.write_pixel_u16(offset, rgb565);
            }
            PixelFormat::Grayscale8 => {
                let gray =
                    ((77 * rgb.0 as u16 + 150 * rgb.1 as u16 + 29 * rgb.2 as u16) >> 8) as u8;
                let _ = self.fb.write_bytes_at(offset, &[gray]);
            }
        }
    }
}

/// Global terminal instance.
static TERMINAL: Mutex<Option<Terminal<FramebufferDisplay>>> = Mutex::new(None);

fn ensure_terminal(term_guard: &mut Option<Terminal<FramebufferDisplay>>) {
    if term_guard.is_none() {
        if let Some(fb) = get_framebuffer() {
            let display = FramebufferDisplay::new(fb);
            let mut terminal = Terminal::new(display, Box::new(ConsoleFont::new()));
            terminal.set_pty_writer(Box::new(|resp: &str| {
                for b in resp.bytes() {
                    crate::drivers::char::tty::queue_input(b);
                }
            }));
            // Default to steady block cursor (DECSCUSR 2)
            terminal.process(b"\x1b[2 q");
            *term_guard = Some(terminal);
            ostd::info!("TTY Terminal: Initialized with os-terminal and ConsoleFont");
        }
    }
}

/// Initializes the framebuffer terminal if framebuffer is available.
pub fn init() {
    let mut term_guard = TERMINAL.lock();
    ensure_terminal(&mut term_guard);
}

/// Writes byte stream into the terminal emulator.
pub fn write_output(data: &[u8]) {
    let mut term_guard = TERMINAL.lock();
    ensure_terminal(&mut term_guard);
    if let Some(terminal) = term_guard.as_mut() {
        terminal.process(data);
    }
}

/// Returns the current terminal window dimensions.
pub fn get_winsize() -> winsize {
    let mut term_guard = TERMINAL.lock();
    ensure_terminal(&mut term_guard);
    if let Some(terminal) = term_guard.as_ref() {
        let (xpix, ypix) = if let Some(fb) = get_framebuffer() {
            (fb.width as u16, fb.height as u16)
        } else {
            (1280, 800)
        };
        winsize {
            ws_row: terminal.rows() as u16,
            ws_col: terminal.columns() as u16,
            ws_xpixel: xpix,
            ws_ypixel: ypix,
        }
    } else {
        winsize {
            ws_row: 25,
            ws_col: 80,
            ws_xpixel: 1280,
            ws_ypixel: 800,
        }
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use alloc::{boxed::Box, format, string::String};
    use ostd::prelude::ktest;

    struct TestDisplay {
        width: usize,
        height: usize,
    }

    impl DrawTarget for TestDisplay {
        fn size(&self) -> (usize, usize) {
            (self.width, self.height)
        }

        fn draw_pixel(&mut self, _x: usize, _y: usize, _rgb: Rgb) {}
    }

    fn create_test_terminal(last_reply: Arc<Mutex<Option<String>>>) -> Terminal<TestDisplay> {
        let display = TestDisplay {
            width: 1280,
            height: 800,
        };
        let mut term = Terminal::new(display, Box::new(ConsoleFont::new()));
        let reply_sink = Arc::clone(&last_reply);
        term.set_pty_writer(Box::new(move |s: &str| {
            *reply_sink.lock() = Some(String::from(s));
        }));
        term.process(b"\x1b[4 q");
        term
    }

    fn query_cursor(
        term: &mut Terminal<TestDisplay>,
        last_reply: &Arc<Mutex<Option<String>>>,
    ) -> (usize, usize) {
        *last_reply.lock() = None;
        term.process(b"\x1b[6n");
        let rep = last_reply
            .lock()
            .clone()
            .expect("expected cursor report response");
        let s = rep.trim_start_matches("\x1b[").trim_end_matches('R');
        let mut parts = s.split(';');
        let row: usize = parts.next().unwrap().parse().unwrap();
        let col: usize = parts.next().unwrap().parse().unwrap();
        (col - 1, row - 1)
    }

    #[ktest]
    fn test_single_step_horizontal_navigation() {
        let reply = Arc::new(Mutex::new(None));
        let mut term = create_test_terminal(Arc::clone(&reply));

        term.process(b"abcdef");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, 6);
        assert_eq!(row, 0);

        // Left Arrow 3 times (\x1b[D): 6 -> 5 -> 4 -> 3
        term.process(b"\x1b[D");
        let (col, _) = query_cursor(&mut term, &reply);
        assert_eq!(col, 5);

        term.process(b"\x1b[D");
        let (col, _) = query_cursor(&mut term, &reply);
        assert_eq!(col, 4);

        term.process(b"\x1b[D");
        let (col, _) = query_cursor(&mut term, &reply);
        assert_eq!(col, 3);

        // Right Arrow 2 times (\x1b[C): 3 -> 4 -> 5
        term.process(b"\x1b[C");
        let (col, _) = query_cursor(&mut term, &reply);
        assert_eq!(col, 4);

        term.process(b"\x1b[C");
        let (col, _) = query_cursor(&mut term, &reply);
        assert_eq!(col, 5);
    }

    #[ktest]
    fn test_ansi_absolute_addressing() {
        let reply = Arc::new(Mutex::new(None));
        let mut term = create_test_terminal(Arc::clone(&reply));

        // \x1b[1;1H -> (col = 0, row = 0)
        term.process(b"\x1b[1;1H");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, 0);
        assert_eq!(row, 0);

        // \x1b[5;10H -> (col = 9, row = 4)
        term.process(b"\x1b[5;10H");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, 9);
        assert_eq!(row, 4);

        // Missing parameters default to 1 (0-based 0)
        // \x1b[;5H -> row default 1 (0), col 5 (4)
        term.process(b"\x1b[;5H");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, 4);
        assert_eq!(row, 0);

        // \x1b[H -> row default 1 (0), col default 1 (0)
        term.process(b"\x1b[H");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, 0);
        assert_eq!(row, 0);
    }

    #[ktest]
    fn test_shell_history_cycling() {
        let reply = Arc::new(Mutex::new(None));
        let mut term = create_test_terminal(Arc::clone(&reply));

        // prompt petraos / $ (12 chars)
        term.process(b"petraos / $ ");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, 12);
        assert_eq!(row, 0);

        // type command: apk install neofetch (20 chars) -> col = 32
        term.process(b"apk install neofetch");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, 32);
        assert_eq!(row, 0);

        // Carriage Return (\r): resets col to 0 on the SAME row
        term.process(b"\r");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, 0);
        assert_eq!(row, 0);

        // redraw prompt
        term.process(b"petraos / $ ");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, 12);
        assert_eq!(row, 0);

        // Erase in line (\x1b[K): clears from col to ws_col - 1, leaves col at current col
        term.process(b"\x1b[K");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, 12);
        assert_eq!(row, 0);
    }

    #[ktest]
    fn test_boundary_clamping() {
        let reply = Arc::new(Mutex::new(None));
        let mut term = create_test_terminal(Arc::clone(&reply));

        let ws_cols = term.columns();

        // Move cursor to col = 0
        term.process(b"\x1b[1;1H");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, 0);
        assert_eq!(row, 0);

        // Left (\x1b[D) at col = 0 clamps at 0
        term.process(b"\x1b[D");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, 0);
        assert_eq!(row, 0);

        // Backspace (\x08) at col = 0 clamps at 0
        term.process(b"\x08");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, 0);
        assert_eq!(row, 0);

        // Move cursor to right edge (col = ws_cols - 1)
        term.process(format!("\x1b[1;{}H", ws_cols).as_bytes());
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, ws_cols - 1);
        assert_eq!(row, 0);

        // Right (\x1b[C) at right edge clamps at ws_cols - 1
        term.process(b"\x1b[C");
        let (col, row) = query_cursor(&mut term, &reply);
        assert_eq!(col, ws_cols - 1);
        assert_eq!(row, 0);
    }
}
