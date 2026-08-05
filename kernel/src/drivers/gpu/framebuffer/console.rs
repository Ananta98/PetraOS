use crate::drivers::char::{CharDevice, InputBuffer, register_char_device};
use crate::drivers::gpu::framebuffer::{Color, font, framebuffer};
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::sync::SpinLock;
use spin::Once;

/// Global reference to the active Framebuffer Console instance.
static FBCON: Once<Arc<FbConsole>> = Once::new();

/// Return a reference to the global framebuffer console driver instance.
pub fn fb_console() -> Option<Arc<FbConsole>> {
    FBCON.get().cloned()
}

/// ANSI escape sequence parser state.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AnsiState {
    Ground,
    Escape,
    Csi,
}

/// Internal state tracking for the graphical Framebuffer Terminal Console.
struct FbConsoleInner {
    fg_color: Color,
    bg_color: Color,
    ansi_state: AnsiState,
    ansi_param_buf: Vec<u8>,
}

/// A character device implementing a graphical framebuffer terminal console.
pub struct FbConsole {
    input: InputBuffer,
    inner: SpinLock<FbConsoleInner>,
}

impl FbConsole {
    /// Create a new Framebuffer Console.
    pub fn new() -> Self {
        Self {
            input: InputBuffer::new(4096),
            inner: SpinLock::new(FbConsoleInner {
                fg_color: Color::WHITE,
                bg_color: Color::BLACK,
                ansi_state: AnsiState::Ground,
                ansi_param_buf: Vec::new(),
            }),
        }
    }

    /// Push raw input bytes (e.g. from keyboard interrupt handlers) into the console input buffer.
    pub fn push_input(&self, data: &[u8]) {
        self.input.push(data);
    }

    /// Get console dimensions in characters (columns, rows).
    pub fn dimensions(&self) -> (u32, u32) {
        if let Some(fb) = framebuffer() {
            let mode = fb.mode();
            (
                mode.width / font::FONT_WIDTH as u32,
                mode.height / font::FONT_HEIGHT as u32,
            )
        } else {
            (80, 25)
        }
    }

    /// Render text bytes directly to the graphical framebuffer screen memory.
    pub fn write_bytes(&self, buf: &[u8]) {
        let Some(fb) = framebuffer() else {
            return;
        };

        let font_w = font::FONT_WIDTH as u32;
        let font_h = font::FONT_HEIGHT as u32;

        let mut inner = self.inner.lock();

        for &byte in buf {
            match inner.ansi_state {
                AnsiState::Ground => match byte {
                    0x1b => {
                        inner.ansi_state = AnsiState::Escape;
                    }
                    b'\n' => {
                        let cx = 0;
                        let cy = fb.cursor_y.load(core::sync::atomic::Ordering::Relaxed) + font_h;
                        fb.cursor_x.store(cx, core::sync::atomic::Ordering::Relaxed);
                        if cy + font_h > fb.mode().height {
                            fb.scroll_up(font_h, inner.bg_color);
                            fb.cursor_y
                                .store(fb.mode().height - font_h, core::sync::atomic::Ordering::Relaxed);
                        } else {
                            fb.cursor_y.store(cy, core::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    b'\r' => {
                        fb.cursor_x.store(0, core::sync::atomic::Ordering::Relaxed);
                    }
                    b'\t' => {
                        let mut cx = fb.cursor_x.load(core::sync::atomic::Ordering::Relaxed);
                        cx = (cx + 8 * font_w) & !(7 * font_w);
                        if cx >= fb.mode().width {
                            cx = 0;
                            let cy = fb.cursor_y.load(core::sync::atomic::Ordering::Relaxed) + font_h;
                            fb.cursor_y.store(cy, core::sync::atomic::Ordering::Relaxed);
                        }
                        fb.cursor_x.store(cx, core::sync::atomic::Ordering::Relaxed);
                    }
                    0x08 | 0x7f => {
                        let mut cx = fb.cursor_x.load(core::sync::atomic::Ordering::Relaxed);
                        let cy = fb.cursor_y.load(core::sync::atomic::Ordering::Relaxed);
                        if cx >= font_w {
                            cx -= font_w;
                            fb.draw_rect(cx, cy, font_w, font_h, inner.bg_color);
                            fb.cursor_x.store(cx, core::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    ch if ch >= 0x20 => {
                        let mut cx = fb.cursor_x.load(core::sync::atomic::Ordering::Relaxed);
                        let mut cy = fb.cursor_y.load(core::sync::atomic::Ordering::Relaxed);

                        if cx + font_w > fb.mode().width {
                            cx = 0;
                            cy += font_h;
                        }

                        if cy + font_h > fb.mode().height {
                            fb.scroll_up(font_h, inner.bg_color);
                            cy = fb.mode().height - font_h;
                        }

                        fb.draw_char_bg(cx, cy, ch as char, inner.fg_color, inner.bg_color);
                        cx += font_w;

                        fb.cursor_x.store(cx, core::sync::atomic::Ordering::Relaxed);
                        fb.cursor_y.store(cy, core::sync::atomic::Ordering::Relaxed);
                    }
                    _ => {}
                },
                AnsiState::Escape => {
                    if byte == b'[' {
                        inner.ansi_state = AnsiState::Csi;
                        inner.ansi_param_buf.clear();
                    } else {
                        inner.ansi_state = AnsiState::Ground;
                    }
                }
                AnsiState::Csi => match byte {
                    b'0'..=b'9' | b';' => {
                        inner.ansi_param_buf.push(byte);
                    }
                    b'm' => {
                        Self::parse_sgr(&inner.ansi_param_buf.clone(), &mut inner);
                        inner.ansi_state = AnsiState::Ground;
                    }
                    b'J' => {
                        fb.clear(inner.bg_color);
                        fb.cursor_x.store(0, core::sync::atomic::Ordering::Relaxed);
                        fb.cursor_y.store(0, core::sync::atomic::Ordering::Relaxed);
                        inner.ansi_state = AnsiState::Ground;
                    }
                    b'H' => {
                        fb.cursor_x.store(0, core::sync::atomic::Ordering::Relaxed);
                        fb.cursor_y.store(0, core::sync::atomic::Ordering::Relaxed);
                        inner.ansi_state = AnsiState::Ground;
                    }
                    _ => {
                        inner.ansi_state = AnsiState::Ground;
                    }
                },
            }
        }
    }

    fn parse_sgr(buf: &[u8], inner: &mut FbConsoleInner) {
        if buf.is_empty() {
            inner.fg_color = Color::WHITE;
            inner.bg_color = Color::BLACK;
            return;
        }

        let s = core::str::from_utf8(buf).unwrap_or("");
        for part in s.split(';') {
            let code = part.parse::<u32>().unwrap_or(0);
            match code {
                0 => {
                    inner.fg_color = Color::WHITE;
                    inner.bg_color = Color::BLACK;
                }
                30 => inner.fg_color = Color::BLACK,
                31 => inner.fg_color = Color::RED,
                32 => inner.fg_color = Color::GREEN,
                33 => inner.fg_color = Color::YELLOW,
                34 => inner.fg_color = Color::BLUE,
                35 => inner.fg_color = Color::MAGENTA,
                36 => inner.fg_color = Color::CYAN,
                37 => inner.fg_color = Color::WHITE,
                39 => inner.fg_color = Color::WHITE,
                40 => inner.bg_color = Color::BLACK,
                41 => inner.bg_color = Color::RED,
                42 => inner.bg_color = Color::GREEN,
                43 => inner.bg_color = Color::YELLOW,
                44 => inner.bg_color = Color::BLUE,
                45 => inner.bg_color = Color::MAGENTA,
                46 => inner.bg_color = Color::CYAN,
                47 => inner.bg_color = Color::WHITE,
                49 => inner.bg_color = Color::BLACK,
                _ => {}
            }
        }
    }
}

impl Default for FbConsole {
    fn default() -> Self {
        Self::new()
    }
}

impl CharDevice for FbConsole {
    fn read(&self, buf: &mut [u8]) -> Result<usize, ostd::Error> {
        self.input.read_into(buf)
    }

    fn write(&self, buf: &[u8]) -> Result<usize, ostd::Error> {
        self.write_bytes(buf);
        Ok(buf.len())
    }
}

/// Driver wrapper for registering the Framebuffer Console with the kernel device manager.
pub struct FbConsoleDriver;

impl crate::device::Driver for FbConsoleDriver {
    fn name(&self) -> &str {
        "fbcon"
    }

    fn bus_name(&self) -> &str {
        "virtual"
    }

    fn description(&self) -> &str {
        "Framebuffer Console TTY Device Driver"
    }

    fn probe(&self) -> Result<(), ostd::Error> {
        let console = Arc::new(FbConsole::new());
        FBCON.call_once(|| console.clone());
        let _ = register_char_device("fbcon", console.clone());
        let _ = register_char_device("tty0", console.clone());
        let _ = register_char_device("tty", console);
        Ok(())
    }
}

crate::module_driver!(
    FBCON_INITCALL,
    fbcon_driver_init,
    "fbcon",
    FbConsoleDriver
);
