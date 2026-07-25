use crate::drivers::gpu::framebuffer::color::{Color, PixelFormat, VideoMode};
use crate::drivers::gpu::framebuffer::font;
use alloc::vec::Vec;
use ostd::sync::SpinLock;

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

    /// Draw a character on the screen using the `font8x8` bitmap font.
    pub fn draw_char(&self, x: u32, y: u32, ch: char, color: Color) {
        let Some(bitmap) = font::get_char_bitmap(ch) else {
            return;
        };
        for row in 0..font::FONT_HEIGHT {
            let row_byte = bitmap[row];
            for col in 0..font::FONT_WIDTH {
                if (row_byte & (1 << col)) != 0 {
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
            curr_x += font::FONT_WIDTH as u32;
        }
    }
}
