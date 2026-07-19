use crate::drivers::gpu::{GPU_MANAGER, GpuDriver};
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::sync::SpinLock;
use spin::Once;

/// Represents pixel storage and layout configurations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 24-bit RGB pixel format.
    Rgb888,
    /// 24-bit BGR pixel format.
    Bgr888,
    /// 32-bit RGBA pixel format.
    Rgba8888,
    /// 32-bit BGRA pixel format.
    Bgra8888,
}

/// Represents the display metrics and format of a video device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoMode {
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u32,
    pub format: PixelFormat,
}

/// Represents a 32-bit RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const RED: Self = Self {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const GREEN: Self = Self {
        r: 0,
        g: 255,
        b: 0,
        a: 255,
    };
    pub const BLUE: Self = Self {
        r: 0,
        g: 0,
        b: 255,
        a: 255,
    };
}

/// A generic software-managed Framebuffer containing video mode metrics
/// and a lock-protected raw pixel buffer.
pub struct Framebuffer {
    pub(crate) mode: VideoMode,
    pub pixels: SpinLock<Vec<u8>>,
}

impl Framebuffer {
    /// Create a new generic framebuffer initialized to black.
    pub fn new(mode: VideoMode) -> Self {
        let size = (mode.pitch as usize) * (mode.height as usize);
        Self {
            mode,
            pixels: SpinLock::new(alloc::vec![0u8; size]),
        }
    }

    /// Returns the video mode metrics of the framebuffer.
    pub fn mode(&self) -> VideoMode {
        self.mode
    }

    /// Clear the screen with a specific color.
    pub fn clear(&self, color: Color) {
        let mut p = self.pixels.lock();
        let bpp_bytes = (self.mode.bpp / 8) as usize;
        if bpp_bytes == 4 {
            for pixel in p.chunks_exact_mut(4) {
                match self.mode.format {
                    PixelFormat::Rgba8888 => {
                        pixel[0] = color.r;
                        pixel[1] = color.g;
                        pixel[2] = color.b;
                        pixel[3] = color.a;
                    }
                    PixelFormat::Bgra8888 => {
                        pixel[0] = color.b;
                        pixel[1] = color.g;
                        pixel[2] = color.r;
                        pixel[3] = color.a;
                    }
                    _ => {
                        pixel[0] = color.r;
                        pixel[1] = color.g;
                        pixel[2] = color.b;
                        pixel[3] = color.a;
                    }
                }
            }
        } else if bpp_bytes == 3 {
            for pixel in p.chunks_exact_mut(3) {
                match self.mode.format {
                    PixelFormat::Rgb888 => {
                        pixel[0] = color.r;
                        pixel[1] = color.g;
                        pixel[2] = color.b;
                    }
                    PixelFormat::Bgr888 => {
                        pixel[0] = color.b;
                        pixel[1] = color.g;
                        pixel[2] = color.r;
                    }
                    _ => {
                        pixel[0] = color.r;
                        pixel[1] = color.g;
                        pixel[2] = color.b;
                    }
                }
            }
        }
    }

    /// Draw a single pixel at (x, y) with a color.
    pub fn draw_pixel(&self, x: u32, y: u32, color: Color) {
        if x >= self.mode.width || y >= self.mode.height {
            return;
        }
        let bpp_bytes = (self.mode.bpp / 8) as usize;
        let offset = (y as usize * self.mode.pitch as usize) + (x as usize * bpp_bytes);
        let mut p = self.pixels.lock();
        if offset + bpp_bytes <= p.len() {
            if bpp_bytes == 4 {
                match self.mode.format {
                    PixelFormat::Rgba8888 => {
                        p[offset] = color.r;
                        p[offset + 1] = color.g;
                        p[offset + 2] = color.b;
                        p[offset + 3] = color.a;
                    }
                    PixelFormat::Bgra8888 => {
                        p[offset] = color.b;
                        p[offset + 1] = color.g;
                        p[offset + 2] = color.r;
                        p[offset + 3] = color.a;
                    }
                    _ => {
                        p[offset] = color.r;
                        p[offset + 1] = color.g;
                        p[offset + 2] = color.b;
                        p[offset + 3] = color.a;
                    }
                }
            } else if bpp_bytes == 3 {
                match self.mode.format {
                    PixelFormat::Rgb888 => {
                        p[offset] = color.r;
                        p[offset + 1] = color.g;
                        p[offset + 2] = color.b;
                    }
                    PixelFormat::Bgr888 => {
                        p[offset] = color.b;
                        p[offset + 1] = color.g;
                        p[offset + 2] = color.r;
                    }
                    _ => {
                        p[offset] = color.r;
                        p[offset + 1] = color.g;
                        p[offset + 2] = color.b;
                    }
                }
            }
        }
    }

    /// Draw a solid rectangle.
    pub fn draw_rect(&self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        for row in y..core::cmp::min(y + h, self.mode.height) {
            for col in x..core::cmp::min(x + w, self.mode.width) {
                self.draw_pixel(col, row, color);
            }
        }
    }

    /// Draw a line from (x0, y0) to (x1, y1) with a color.
    pub fn draw_line(&self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        let mut x = x0;
        let mut y = y0;

        loop {
            if x >= 0 && x < self.mode.width as i32 && y >= 0 && y < self.mode.height as i32 {
                self.draw_pixel(x as u32, y as u32, color);
            }
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                if x == x1 {
                    break;
                }
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                if y == y1 {
                    break;
                }
                err += dx;
                y += sy;
            }
        }
    }

    /// Draw a circle centered at (xc, yc) with radius r and a color.
    pub fn draw_circle(&self, xc: i32, yc: i32, r: i32, color: Color) {
        let mut x = 0;
        let mut y = r;
        let mut d = 3 - 2 * r;

        let draw_symmetric = |x_val: i32, y_val: i32| {
            let points = [
                (xc + x_val, yc + y_val),
                (xc - x_val, yc + y_val),
                (xc + x_val, yc - y_val),
                (xc - x_val, yc - y_val),
                (xc + y_val, yc + x_val),
                (xc - y_val, yc + x_val),
                (xc + y_val, yc - x_val),
                (xc - y_val, yc - x_val),
            ];
            for &(px, py) in &points {
                if px >= 0 && px < self.mode.width as i32 && py >= 0 && py < self.mode.height as i32
                {
                    self.draw_pixel(px as u32, py as u32, color);
                }
            }
        };

        draw_symmetric(x, y);
        while y >= x {
            x += 1;
            if d > 0 {
                y -= 1;
                d = d + 4 * (x - y) + 10;
            } else {
                d = d + 4 * x + 6;
            }
            draw_symmetric(x, y);
        }
    }

    /// Draw an ASCII character on the screen using a simple built-in 8x8 font.
    pub fn draw_char(&self, x: u32, y: u32, ch: char, color: Color) {
        let c = ch as usize;
        if c < 32 || c > 126 {
            return;
        }
        let font_idx = c - 32;
        let bitmap = SIMPLE_FONT[font_idx];
        for row in 0..8 {
            let row_byte = bitmap[row];
            for col in 0..8 {
                if (row_byte & (1 << (7 - col))) != 0 {
                    self.draw_pixel(x + col as u32, y + row as u32, color);
                }
            }
        }
    }

    /// Draw a text string on the screen.
    pub fn draw_string(&self, x: u32, y: u32, s: &str, color: Color) {
        let mut curr_x = x;
        for ch in s.chars() {
            if ch == '\n' {
                continue;
            }
            self.draw_char(curr_x, y, ch, color);
            curr_x += 8;
        }
    }
}

impl GpuDriver for Framebuffer {
    fn name(&self) -> &str {
        "framebuffer"
    }

    fn current_mode(&self) -> VideoMode {
        self.mode
    }

    fn set_mode(&self, mode: VideoMode) -> Result<(), ostd::Error> {
        if mode == self.mode {
            Ok(())
        } else {
            Err(ostd::Error::InvalidArgs)
        }
    }

    fn supported_modes(&self) -> &[VideoMode] {
        core::slice::from_ref(&self.mode)
    }

    fn framebuffer(&self) -> Arc<Framebuffer> {
        framebuffer().expect("framebuffer not initialized")
    }
}

pub static FRAMEBUFFER: Once<Arc<Framebuffer>> = Once::new();

/// Get a reference to the active framebuffer instance.
pub fn framebuffer() -> Option<Arc<Framebuffer>> {
    FRAMEBUFFER.get().cloned()
}

/// Initialize the framebuffer and register it as a GPU driver.
pub fn init() {
    let mode = VideoMode {
        width: 1024,
        height: 768,
        pitch: 1024 * 4,
        bpp: 32,
        format: PixelFormat::Rgba8888,
    };
    let fb = Arc::new(Framebuffer::new(mode));
    FRAMEBUFFER.call_once(|| fb.clone());

    GPU_MANAGER
        .register_driver(fb)
        .expect("failed to register framebuffer driver");
}

// Simple built-in 8x8 font bitmap for ASCII characters 32 to 126
const SIMPLE_FONT: [[u8; 8]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // ' ' (32)
    [0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x00], // '!'
    [0x36, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // '"'
    [0x36, 0x36, 0x7f, 0x36, 0x7f, 0x36, 0x36, 0x00], // '#'
    [0x0c, 0x3e, 0x03, 0x1e, 0x30, 0x1f, 0x0c, 0x00], // '$'
    [0x00, 0x63, 0x30, 0x18, 0x0c, 0x06, 0x63, 0x00], // '%'
    [0x1c, 0x36, 0x1c, 0x3b, 0x6e, 0x36, 0x1d, 0x00], // '&'
    [0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // '\''
    [0x0c, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0c, 0x00], // '('
    [0x30, 0x18, 0x0c, 0x0c, 0x0c, 0x18, 0x30, 0x00], // ')'
    [0x00, 0x66, 0x3c, 0xff, 0x3c, 0x66, 0x00, 0x00], // '*'
    [0x00, 0x18, 0x18, 0x7e, 0x18, 0x18, 0x00, 0x00], // '+'
    [0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x0c, 0x00], // ','
    [0x00, 0x00, 0x00, 0x7e, 0x00, 0x00, 0x00, 0x00], // '-'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00], // '.'
    [0x00, 0x43, 0x30, 0x18, 0x0c, 0x06, 0x61, 0x00], // '/'
    [0x3e, 0x63, 0x67, 0x6f, 0x7b, 0x63, 0x3e, 0x00], // '0'
    [0x0c, 0x1c, 0x0c, 0x0c, 0x0c, 0x0c, 0x3f, 0x00], // '1'
    [0x3e, 0x63, 0x06, 0x1c, 0x30, 0x60, 0x7f, 0x00], // '2'
    [0x7f, 0x06, 0x0c, 0x1e, 0x03, 0x63, 0x3e, 0x00], // '3'
    [0x06, 0x0e, 0x1e, 0x36, 0x7f, 0x06, 0x06, 0x00], // '4'
    [0x7f, 0x60, 0x7e, 0x03, 0x03, 0x63, 0x3e, 0x00], // '5'
    [0x1c, 0x30, 0x60, 0x7e, 0x63, 0x63, 0x3e, 0x00], // '6'
    [0x7f, 0x03, 0x06, 0x0c, 0x18, 0x18, 0x18, 0x00], // '7'
    [0x3e, 0x63, 0x63, 0x3e, 0x63, 0x63, 0x3e, 0x00], // '8'
    [0x3e, 0x63, 0x63, 0x7f, 0x03, 0x06, 0x38, 0x00], // '9'
    [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00], // ':'
    [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x0c, 0x00], // ';'
    [0x06, 0x0c, 0x18, 0x30, 0x18, 0x0c, 0x06, 0x00], // '<'
    [0x00, 0x00, 0x7e, 0x00, 0x7e, 0x00, 0x00, 0x00], // '='
    [0x30, 0x18, 0x0c, 0x06, 0x0c, 0x18, 0x30, 0x00], // '>'
    [0x3e, 0x63, 0x06, 0x0c, 0x18, 0x00, 0x18, 0x00], // '?'
    [0x3e, 0x63, 0x6b, 0x6b, 0x6b, 0x60, 0x3e, 0x00], // '@'
    [0x18, 0x3c, 0x66, 0x66, 0x7e, 0x66, 0x66, 0x00], // 'A'
    [0x7e, 0x33, 0x33, 0x3e, 0x33, 0x33, 0x7e, 0x00], // 'B'
    [0x1e, 0x33, 0x60, 0x60, 0x60, 0x33, 0x1e, 0x00], // 'C'
    [0x7c, 0x36, 0x33, 0x33, 0x33, 0x36, 0x7c, 0x00], // 'D'
    [0x7f, 0x60, 0x60, 0x78, 0x60, 0x60, 0x7f, 0x00], // 'E'
    [0x7f, 0x60, 0x60, 0x78, 0x60, 0x60, 0x60, 0x00], // 'F'
    [0x3e, 0x63, 0x60, 0x6f, 0x63, 0x63, 0x3e, 0x00], // 'G'
    [0x66, 0x66, 0x66, 0x7e, 0x66, 0x66, 0x66, 0x00], // 'H'
    [0x3c, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3c, 0x00], // 'I'
    [0x0f, 0x06, 0x06, 0x06, 0x06, 0x66, 0x3c, 0x00], // 'J'
    [0x66, 0x6c, 0x78, 0x70, 0x78, 0x6c, 0x66, 0x00], // 'K'
    [0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7f, 0x00], // 'L'
    [0x63, 0x77, 0x7f, 0x6b, 0x63, 0x63, 0x63, 0x00], // 'M'
    [0x63, 0x63, 0x67, 0x6f, 0x7b, 0x73, 0x63, 0x00], // 'N'
    [0x3e, 0x63, 0x63, 0x63, 0x63, 0x63, 0x3e, 0x00], // 'O'
    [0x7e, 0x63, 0x63, 0x7e, 0x60, 0x60, 0x60, 0x00], // 'P'
    [0x3e, 0x63, 0x63, 0x63, 0x6b, 0x66, 0x3d, 0x00], // 'Q'
    [0x7e, 0x63, 0x63, 0x7e, 0x78, 0x6c, 0x66, 0x00], // 'R'
    [0x3e, 0x63, 0x60, 0x3e, 0x03, 0x63, 0x3e, 0x00], // 'S'
    [0x7e, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00], // 'T'
    [0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x00], // 'U'
    [0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x18, 0x00], // 'V'
    [0x63, 0x63, 0x63, 0x6b, 0x7f, 0x77, 0x63, 0x00], // 'W'
    [0x66, 0x66, 0x3c, 0x18, 0x3c, 0x66, 0x66, 0x00], // 'X'
    [0x66, 0x66, 0x66, 0x3c, 0x18, 0x18, 0x18, 0x00], // 'Y'
    [0x7f, 0x03, 0x06, 0x0c, 0x18, 0x30, 0x7f, 0x00], // 'Z'
    [0x3c, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3c, 0x00], // '['
    [0x00, 0x61, 0x06, 0x0c, 0x18, 0x30, 0x43, 0x00], // '\\'
    [0x3c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x3c, 0x00], // ']'
    [0x08, 0x1c, 0x36, 0x63, 0x00, 0x00, 0x00, 0x00], // '^'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff], // '_'
    [0x18, 0x0c, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00], // '`'
    [0x00, 0x00, 0x3c, 0x03, 0x3f, 0x63, 0x3f, 0x00], // 'a'
    [0x60, 0x60, 0x7e, 0x63, 0x63, 0x63, 0x7e, 0x00], // 'b'
    [0x00, 0x00, 0x3e, 0x60, 0x60, 0x63, 0x3e, 0x00], // 'c'
    [0x03, 0x03, 0x3f, 0x63, 0x63, 0x63, 0x3f, 0x00], // 'd'
    [0x00, 0x00, 0x3e, 0x63, 0x7f, 0x60, 0x3e, 0x00], // 'e'
    [0x1c, 0x36, 0x30, 0x78, 0x30, 0x30, 0x30, 0x00], // 'f'
    [0x00, 0x00, 0x3f, 0x63, 0x63, 0x3f, 0x03, 0x3e], // 'g'
    [0x60, 0x60, 0x7e, 0x63, 0x63, 0x63, 0x63, 0x00], // 'h'
    [0x18, 0x00, 0x18, 0x18, 0x18, 0x18, 0x3c, 0x00], // 'i'
    [0x06, 0x00, 0x06, 0x06, 0x06, 0x06, 0x06, 0x3c], // 'j'
    [0x60, 0x60, 0x66, 0x6c, 0x78, 0x6c, 0x66, 0x00], // 'k'
    [0x30, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3c, 0x00], // 'l'
    [0x00, 0x00, 0x6b, 0x7f, 0x6b, 0x63, 0x63, 0x00], // 'm'
    [0x00, 0x00, 0x7e, 0x63, 0x63, 0x63, 0x63, 0x00], // 'n'
    [0x00, 0x00, 0x3e, 0x63, 0x63, 0x63, 0x3e, 0x00], // 'o'
    [0x00, 0x00, 0x7e, 0x63, 0x63, 0x7e, 0x60, 0x60], // 'p'
    [0x00, 0x00, 0x3f, 0x63, 0x63, 0x3f, 0x03, 0x03], // 'q'
    [0x00, 0x00, 0x7e, 0x63, 0x60, 0x60, 0x60, 0x00], // 'r'
    [0x00, 0x00, 0x3e, 0x60, 0x3e, 0x03, 0x3e, 0x00], // 's'
    [0x30, 0x30, 0x7c, 0x30, 0x30, 0x30, 0x1e, 0x00], // 't'
    [0x00, 0x00, 0x63, 0x63, 0x63, 0x63, 0x3f, 0x00], // 'u'
    [0x00, 0x00, 0x63, 0x63, 0x63, 0x3c, 0x18, 0x00], // 'v'
    [0x00, 0x00, 0x63, 0x63, 0x6b, 0x7f, 0x36, 0x00], // 'w'
    [0x00, 0x00, 0x63, 0x3c, 0x18, 0x3c, 0x63, 0x00], // 'x'
    [0x00, 0x00, 0x63, 0x63, 0x63, 0x3f, 0x03, 0x3e], // 'y'
    [0x00, 0x00, 0x7f, 0x0c, 0x18, 0x30, 0x7f, 0x00], // 'z'
    [0x0c, 0x18, 0x18, 0x30, 0x18, 0x18, 0x0c, 0x00], // '{'
    [0x18, 0x18, 0x18, 0x00, 0x18, 0x18, 0x18, 0x00], // '|'
    [0x30, 0x18, 0x18, 0x0c, 0x18, 0x18, 0x30, 0x00], // '}'
    [0x00, 0x00, 0x3c, 0x66, 0x03, 0x00, 0x00, 0x00], // '~'
];

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_graphics_basic() {
        let mode = VideoMode {
            width: 100,
            height: 100,
            pitch: 400,
            bpp: 32,
            format: PixelFormat::Rgba8888,
        };
        let fb = Framebuffer::new(mode);

        // 1. Initialized to 0
        {
            let pixels = fb.pixels.lock();
            assert_eq!(pixels[0], 0);
            assert_eq!(pixels[pixels.len() - 1], 0);
        }

        // 2. Test draw_pixel
        let red = Color::RED;
        fb.draw_pixel(10, 20, red);
        {
            let pixels = fb.pixels.lock();
            let offset = (20 * mode.pitch as usize) + (10 * 4);
            assert_eq!(pixels[offset], red.r);
            assert_eq!(pixels[offset + 1], red.g);
            assert_eq!(pixels[offset + 2], red.b);
            assert_eq!(pixels[offset + 3], red.a);
        }

        // 3. Test clear
        let blue = Color::BLUE;
        fb.clear(blue);
        {
            let pixels = fb.pixels.lock();
            assert_eq!(pixels[0], blue.r);
            assert_eq!(pixels[1], blue.g);
            assert_eq!(pixels[2], blue.b);
            assert_eq!(pixels[3], blue.a);
        }
    }
}
