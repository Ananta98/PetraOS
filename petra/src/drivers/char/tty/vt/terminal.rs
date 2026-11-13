// SPDX-License-Identifier: MPL-2.0

//! Framebuffer Terminal Console powered by `os-terminal`.
//!
//! Provides a standards-compliant VT100 / XTerm terminal emulator
//! rendering directly to the physical linear framebuffer.

use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use os_terminal::{DrawTarget, Rgb, Terminal};
use ostd::timer::{Jiffies, TIMER_FREQ};
use spin::Mutex;

use super::font::ConsoleFont;

use crate::{
    drivers::drm::framebuffer::{FrameBuffer, PixelFormat, get_framebuffer},
    uapi::termios::winsize,
};

/// RAM-backed double buffer avoiding per-pixel MMIO VM-exits to QEMU/hardware.
///
/// Pixel bytes and dirty-row bitmasks use relaxed atomics so `draw_pixel`
/// never acquires a mutex per pixel; writers are serialized by `TERMINAL`.
pub struct ShadowFb {
    data: Vec<AtomicU8>,
    dirty_rows: Vec<AtomicU64>,
    scratch: Mutex<Vec<u8>>,
}

static SHADOW_FB: Mutex<Option<Arc<ShadowFb>>> = Mutex::new(None);
static NEEDS_FLUSH: AtomicBool = AtomicBool::new(false);
static TIMER_ACTIVE: AtomicBool = AtomicBool::new(false);
static LAST_FLUSH_TICK: AtomicU64 = AtomicU64::new(0);

/// Hardware display target integrating `FrameBuffer` with `os-terminal`'s `DrawTarget`.
pub struct FramebufferDisplay {
    width: usize,
    height: usize,
    line_size: usize,
    pixel_format: PixelFormat,
    bpp: usize,
    shadow: Arc<ShadowFb>,
}

impl FramebufferDisplay {
    pub fn new(fb: Arc<FrameBuffer>, shadow: Arc<ShadowFb>) -> Self {
        let width = fb.width;
        let height = fb.height;
        let line_size = fb.line_size;
        let pixel_format = fb.pixel_format;
        let bpp = pixel_format.nbytes();
        Self {
            width,
            height,
            line_size,
            pixel_format,
            bpp,
            shadow,
        }
    }

    #[inline(always)]
    fn write_pixel_bytes<const N: usize>(&self, offset: usize, bytes: [u8; N], y: usize) {
        let slice = &self.shadow.data[offset..offset + N];
        let mut changed = false;
        for (dst, &src) in slice.iter().zip(bytes.iter()) {
            if dst.load(Ordering::Relaxed) != src {
                dst.store(src, Ordering::Relaxed);
                changed = true;
            }
        }
        if changed {
            let chunk = y / 64;
            if let Some(mask) = self.shadow.dirty_rows.get(chunk) {
                mask.fetch_or(1 << (y % 64), Ordering::Relaxed);
            }
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
        if offset + self.bpp > self.shadow.data.len() {
            return;
        }
        match self.pixel_format {
            PixelFormat::BgrReserved => {
                let pixel = (rgb.2 as u32) | ((rgb.1 as u32) << 8) | ((rgb.0 as u32) << 16);
                self.write_pixel_bytes(offset, pixel.to_ne_bytes(), y);
            }
            PixelFormat::Rgb888 => {
                self.write_pixel_bytes(offset, [rgb.0, rgb.1, rgb.2], y);
            }
            PixelFormat::Rgb565 => {
                let r = (rgb.0 >> 3) as u16;
                let g = (rgb.1 >> 2) as u16;
                let b = (rgb.2 >> 3) as u16;
                self.write_pixel_bytes(offset, ((r << 11) | (g << 5) | b).to_ne_bytes(), y);
            }
            PixelFormat::Grayscale8 => {
                let gray =
                    ((77 * rgb.0 as u16 + 150 * rgb.1 as u16 + 29 * rgb.2 as u16) >> 8) as u8;
                self.write_pixel_bytes(offset, [gray], y);
            }
        }
    }
}

/// Global terminal instance.
static TERMINAL: Mutex<Option<Terminal<FramebufferDisplay>>> = Mutex::new(None);

fn ensure_terminal(term_guard: &mut Option<Terminal<FramebufferDisplay>>) {
    if term_guard.is_none() {
        if let Some(fb) = get_framebuffer() {
            let total_size = fb.total_size;
            let mut initial_data = vec![0u8; total_size];
            let _ = fb.read_bytes(0, &mut initial_data);

            let atomic_data: Vec<AtomicU8> = initial_data.into_iter().map(AtomicU8::new).collect();
            let dirty_len = fb.height.div_ceil(64);
            let dirty_rows = (0..dirty_len).map(|_| AtomicU64::new(0)).collect();

            let shadow = Arc::new(ShadowFb {
                data: atomic_data,
                dirty_rows,
                scratch: Mutex::new(Vec::new()),
            });
            *SHADOW_FB.lock() = Some(Arc::clone(&shadow));

            let display = FramebufferDisplay::new(fb, shadow);
            let mut terminal = Terminal::new(display, Box::new(ConsoleFont::new()));
            terminal.set_auto_flush(false);
            terminal.set_pty_writer(Box::new(|resp: &str| {
                for b in resp.bytes() {
                    crate::drivers::char::tty::queue_input(b);
                }
            }));
            // Default to steady block cursor (DECSCUSR 2)
            terminal.process(b"\x1b[2 q");
            terminal.flush();
            *term_guard = Some(terminal);
            flush_shadow();
        }
    }
}

/// Flushes dirty raster rows from RAM shadow buffer directly to physical framebuffer.
pub fn flush_shadow() {
    let shadow_guard = SHADOW_FB.lock();
    let Some(shadow) = shadow_guard.as_ref() else {
        return;
    };
    let Some(fb) = get_framebuffer() else {
        return;
    };

    let height = fb.height;
    let line_size = fb.line_size;
    let total_size = fb.total_size;
    let mut scratch = shadow.scratch.lock();

    let mut y = 0;
    while y < height {
        let chunk_idx = y / 64;
        let Some(mask_atom) = shadow.dirty_rows.get(chunk_idx) else {
            break;
        };
        let mask = mask_atom.load(Ordering::Relaxed);
        if mask == 0 {
            y = (chunk_idx + 1) * 64;
            continue;
        }
        let bit = y % 64;
        if (mask & (1 << bit)) == 0 {
            y += 1;
            continue;
        }

        let start_y = y;
        while y < height {
            let c = y / 64;
            let Some(m) = shadow.dirty_rows.get(c) else {
                break;
            };
            if (m.load(Ordering::Relaxed) & (1 << (y % 64))) == 0 {
                break;
            }
            y += 1;
        }
        let end_y = y;

        let start = start_y * line_size;
        let end = (end_y * line_size).min(shadow.data.len()).min(total_size);
        if start < end {
            let len = end - start;
            scratch.clear();
            scratch.reserve(len);
            for b in &shadow.data[start..end] {
                scratch.push(b.load(Ordering::Relaxed));
            }
            let _ = fb.write_bytes_at(start, &scratch);
        }
    }
    for row in &shadow.dirty_rows {
        row.store(0, Ordering::Relaxed);
    }
}

/// Timer callback that coalesces rapid terminal writes into ~60 Hz display flushes.
fn tick_flush() {
    TIMER_ACTIVE.store(true, Ordering::Relaxed);
    if !NEEDS_FLUSH.load(Ordering::Acquire) {
        return;
    }

    let now = Jiffies::elapsed().as_u64();
    let min_interval = (TIMER_FREQ / 60).max(1);
    let last = LAST_FLUSH_TICK.load(Ordering::Relaxed);
    if now.saturating_sub(last) < min_interval {
        return;
    }

    let Some(mut term_guard) = TERMINAL.try_lock() else {
        return;
    };
    if !NEEDS_FLUSH.swap(false, Ordering::AcqRel) {
        return;
    }
    LAST_FLUSH_TICK.store(now, Ordering::Relaxed);
    if let Some(terminal) = term_guard.as_mut() {
        terminal.flush();
    }
    drop(term_guard);
    flush_shadow();
}

/// Initializes the framebuffer terminal if framebuffer is available.
pub fn init() {
    static TIMER_REGISTERED: spin::Once<()> = spin::Once::new();
    TIMER_REGISTERED.call_once(|| {
        ostd::timer::register_callback_on_cpu(tick_flush);
    });
    let mut term_guard = TERMINAL.lock();
    ensure_terminal(&mut term_guard);
}

/// Writes byte stream into the terminal emulator.
pub fn write_output(data: &[u8]) {
    let mut term_guard = TERMINAL.lock();
    ensure_terminal(&mut term_guard);
    if let Some(terminal) = term_guard.as_mut() {
        terminal.process(data);
        if !TIMER_ACTIVE.load(Ordering::Relaxed) {
            terminal.flush();
            drop(term_guard);
            flush_shadow();
            return;
        }
        NEEDS_FLUSH.store(true, Ordering::Release);
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
