// SPDX-License-Identifier: MPL-2.0

//! Framebuffer Terminal Console powered by `os-terminal`.
//!
//! Provides a standards-compliant VT100 / XTerm terminal emulator
//! rendering directly to the physical linear framebuffer.

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, Ordering};
use os_terminal::{DrawTarget, Rgb, Terminal};
use spin::Mutex;

use super::font::ConsoleFont;

use crate::{
    drivers::drm::framebuffer::{FrameBuffer, PixelFormat, get_framebuffer},
    uapi::termios::winsize,
};

/// Lock-free RAM-backed double buffer avoiding per-pixel MMIO VM-exits and
/// per-pixel mutex contention, while tracking per-row dirty X-spans.
pub struct ShadowFb {
    /// 32-bit pixel words in internal RAM (`width * height` for 32bpp, empty otherwise).
    pixels32: Vec<AtomicU32>,
    /// Fallback byte buffer for non-32bpp formats.
    bytes_fallback: Mutex<Vec<u8>>,
    dirty_rows: Vec<AtomicU64>,
    dirty_min_x: Vec<AtomicU16>,
    dirty_max_x: Vec<AtomicU16>,
}

static SHADOW_FB: Mutex<Option<Arc<ShadowFb>>> = Mutex::new(None);

impl ShadowFb {
    fn new(width: usize, height: usize, total_size: usize, is_32bpp: bool) -> Self {
        let dirty_len = height.div_ceil(64);
        let pixels32 = if is_32bpp {
            let mut v = Vec::with_capacity(width * height);
            v.resize_with(width * height, || AtomicU32::new(0));
            v
        } else {
            Vec::new()
        };
        let bytes_fallback = if is_32bpp {
            Mutex::new(Vec::new())
        } else {
            Mutex::new(alloc::vec![0u8; total_size])
        };
        let mut dirty_rows = Vec::with_capacity(dirty_len);
        dirty_rows.resize_with(dirty_len, || AtomicU64::new(0));
        let mut dirty_min_x = Vec::with_capacity(height);
        dirty_min_x.resize_with(height, || AtomicU16::new(u16::MAX));
        let mut dirty_max_x = Vec::with_capacity(height);
        dirty_max_x.resize_with(height, || AtomicU16::new(0));
        Self {
            pixels32,
            bytes_fallback,
            dirty_rows,
            dirty_min_x,
            dirty_max_x,
        }
    }

    #[inline(always)]
    fn mark_dirty(&self, x: usize, y: usize) {
        let chunk = y / 64;
        if let Some(mask) = self.dirty_rows.get(chunk) {
            mask.fetch_or(1u64 << (y % 64), Ordering::Relaxed);
        }
        let x16 = x.min(u16::MAX as usize - 1) as u16;
        if let Some(min_x) = self.dirty_min_x.get(y) {
            let cur = min_x.load(Ordering::Relaxed);
            if x16 < cur {
                min_x.store(x16, Ordering::Relaxed);
            }
        }
        let x_end = x16.saturating_add(1);
        if let Some(max_x) = self.dirty_max_x.get(y) {
            let cur = max_x.load(Ordering::Relaxed);
            if x_end > cur {
                max_x.store(x_end, Ordering::Relaxed);
            }
        }
    }
}

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
    fn write_pixel_u32(&self, x: usize, y: usize, pixel: u32) {
        let idx = y * self.width + x;
        if let Some(slot) = self.shadow.pixels32.get(idx) {
            if slot.load(Ordering::Relaxed) != pixel {
                slot.store(pixel, Ordering::Relaxed);
                self.shadow.mark_dirty(x, y);
            }
        }
    }

    #[inline(always)]
    fn write_pixel_bytes<const N: usize>(&self, offset: usize, bytes: [u8; N], x: usize, y: usize) {
        let mut fallback = self.shadow.bytes_fallback.lock();
        if offset + N > fallback.len() {
            return;
        }
        if fallback[offset..offset + N] != bytes {
            fallback[offset..offset + N].copy_from_slice(&bytes);
            self.shadow.mark_dirty(x, y);
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
        match self.pixel_format {
            PixelFormat::BgrReserved => {
                let pixel = (rgb.2 as u32) | ((rgb.1 as u32) << 8) | ((rgb.0 as u32) << 16);
                self.write_pixel_u32(x, y, pixel);
            }
            PixelFormat::Rgb888 => {
                let offset = x * self.bpp + y * self.line_size;
                self.write_pixel_bytes(offset, [rgb.0, rgb.1, rgb.2], x, y);
            }
            PixelFormat::Rgb565 => {
                let offset = x * self.bpp + y * self.line_size;
                let r = (rgb.0 >> 3) as u16;
                let g = (rgb.1 >> 2) as u16;
                let b = (rgb.2 >> 3) as u16;
                self.write_pixel_bytes(offset, ((r << 11) | (g << 5) | b).to_ne_bytes(), x, y);
            }
            PixelFormat::Grayscale8 => {
                let offset = x * self.bpp + y * self.line_size;
                let gray =
                    ((77 * rgb.0 as u16 + 150 * rgb.1 as u16 + 29 * rgb.2 as u16) >> 8) as u8;
                self.write_pixel_bytes(offset, [gray], x, y);
            }
        }
    }
}

/// Global terminal instance.
static TERMINAL: Mutex<Option<Terminal<FramebufferDisplay>>> = Mutex::new(None);

fn ensure_terminal(term_guard: &mut Option<Terminal<FramebufferDisplay>>) {
    if term_guard.is_none() {
        if let Some(fb) = get_framebuffer() {
            let c0 = crate::utils::perf::read_cycles();
            let t0 = crate::utils::perf::now_ns();
            let total_size = fb.total_size;
            let width = fb.width;
            let height = fb.height;
            let is_32bpp = matches!(fb.pixel_format, PixelFormat::BgrReserved);

            // FrameBuffer::probe() already cleared VRAM; zero in internal RAM instead of slow MMIO read.
            let shadow = Arc::new(ShadowFb::new(width, height, total_size, is_32bpp));
            *SHADOW_FB.lock() = Some(Arc::clone(&shadow));
            crate::utils::perf::set_shadow_fb_bytes(total_size);

            let display = FramebufferDisplay::new(fb, shadow);
            let mut terminal = Terminal::new(display, Box::new(ConsoleFont::new()));
            terminal.set_color_cache_size(512);
            terminal.set_pty_writer(Box::new(|resp: &str| {
                for b in resp.bytes() {
                    crate::drivers::char::tty::queue_input(b);
                }
            }));
            // Default to steady block cursor (DECSCUSR 2)
            terminal.process(b"\x1b[2 q");
            *term_guard = Some(terminal);
            flush_shadow();
            let (cycles, us) = crate::utils::perf::elapsed(c0, t0);
            ostd::info!(
                "PERF_CONSOLE_INIT: cycles={}, frame_time_us={}, shadow_bytes={}",
                cycles,
                us,
                total_size
            );
        }
    }
}

/// Flushes only dirty X-spans from the internal RAM shadow buffer to the
/// physical framebuffer in a single top-to-bottom pass (tear-free, no full-screen redraw).
pub fn flush_shadow() {
    let shadow_guard = SHADOW_FB.lock();
    let Some(shadow) = shadow_guard.as_ref() else {
        return;
    };
    let Some(fb) = get_framebuffer() else {
        return;
    };

    let height = fb.height;
    let width = fb.width;
    let line_size = fb.line_size;
    let bpp = fb.pixel_format.nbytes();
    let total_size = fb.total_size;

    if !shadow.pixels32.is_empty() {
        let mut row_buf = [0u8; 8192];
        for (chunk_idx, dirty_word) in shadow.dirty_rows.iter().enumerate() {
            let mut mask = dirty_word.swap(0, Ordering::Relaxed);
            while mask != 0 {
                let bit = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                let y = chunk_idx * 64 + bit;
                if y >= height {
                    break;
                }
                let min_x = shadow.dirty_min_x[y].swap(u16::MAX, Ordering::Relaxed) as usize;
                let max_x = (shadow.dirty_max_x[y].swap(0, Ordering::Relaxed) as usize).min(width);
                if min_x >= max_x {
                    continue;
                }

                let span_pixels = max_x - min_x;
                let span_bytes = span_pixels * 4;
                let row_pixel_base = y * width + min_x;
                let fb_offset = y * line_size + min_x * 4;
                if fb_offset + span_bytes > total_size {
                    continue;
                }

                if span_bytes <= row_buf.len() {
                    for (i, dst) in row_buf[..span_bytes].chunks_exact_mut(4).enumerate() {
                        let px = shadow.pixels32[row_pixel_base + i].load(Ordering::Relaxed);
                        dst.copy_from_slice(&px.to_ne_bytes());
                    }
                    let _ = fb.write_bytes_at(fb_offset, &row_buf[..span_bytes]);
                } else {
                    let mut tmp = alloc::vec![0u8; span_bytes];
                    for (i, dst) in tmp.chunks_exact_mut(4).enumerate() {
                        let px = shadow.pixels32[row_pixel_base + i].load(Ordering::Relaxed);
                        dst.copy_from_slice(&px.to_ne_bytes());
                    }
                    let _ = fb.write_bytes_at(fb_offset, &tmp);
                }
            }
        }
        return;
    }

    let fallback = shadow.bytes_fallback.lock();
    for (chunk_idx, dirty_word) in shadow.dirty_rows.iter().enumerate() {
        let mut mask = dirty_word.swap(0, Ordering::Relaxed);
        while mask != 0 {
            let bit = mask.trailing_zeros() as usize;
            mask &= mask - 1;
            let y = chunk_idx * 64 + bit;
            if y >= height {
                break;
            }
            let min_x = shadow.dirty_min_x[y].swap(u16::MAX, Ordering::Relaxed) as usize;
            let max_x = (shadow.dirty_max_x[y].swap(0, Ordering::Relaxed) as usize).min(width);
            if min_x >= max_x {
                continue;
            }
            let start = y * line_size + min_x * bpp;
            let end = (y * line_size + max_x * bpp)
                .min(fallback.len())
                .min(total_size);
            if start < end {
                let _ = fb.write_bytes_at(start, &fallback[start..end]);
            }
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
    flush_shadow();
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
