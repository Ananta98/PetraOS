//! Framebuffer Graphics Subsystem
//!
//! Provides linear framebuffer access, pixel format handling,
//! drawing primitives, and hardware blitting.

use super::font::{get_glyph, FONT_HEIGHT, FONT_WIDTH};
use crate::sync::spinlock::Spinlock;
use core::ptr;

/// Global primary framebuffer instance.
pub static FRAMEBUFFER: Spinlock<Option<Framebuffer>> = Spinlock::new(None);

/// RGB Color representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const RED: Self = Self::rgb(220, 50, 47);
    pub const GREEN: Self = Self::rgb(133, 153, 0);
    pub const BLUE: Self = Self::rgb(38, 139, 210);
    pub const YELLOW: Self = Self::rgb(181, 137, 0);
    pub const CYAN: Self = Self::rgb(42, 161, 152);
    pub const MAGENTA: Self = Self::rgb(211, 54, 130);
    pub const GRAY: Self = Self::rgb(128, 128, 128);
    pub const DARK_GRAY: Self = Self::rgb(30, 30, 30);
    pub const LIGHT_GRAY: Self = Self::rgb(200, 200, 200);

    pub const BRIGHT_BLACK: Self = Self::rgb(100, 100, 100);
    pub const BRIGHT_RED: Self = Self::rgb(255, 100, 100);
    pub const BRIGHT_GREEN: Self = Self::rgb(100, 255, 100);
    pub const BRIGHT_YELLOW: Self = Self::rgb(255, 255, 100);
    pub const BRIGHT_BLUE: Self = Self::rgb(100, 150, 255);
    pub const BRIGHT_MAGENTA: Self = Self::rgb(255, 100, 255);
    pub const BRIGHT_CYAN: Self = Self::rgb(100, 255, 255);
    pub const BRIGHT_WHITE: Self = Self::rgb(255, 255, 255);

    #[inline(always)]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    #[inline(always)]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    #[inline(always)]
    pub const fn from_u32(val: u32) -> Self {
        Self {
            r: ((val >> 16) & 0xFF) as u8,
            g: ((val >> 8) & 0xFF) as u8,
            b: (val & 0xFF) as u8,
            a: ((val >> 24) & 0xFF) as u8,
        }
    }

    #[inline(always)]
    pub const fn to_u32(self) -> u32 {
        ((self.a as u32) << 24)
            | ((self.r as u32) << 16)
            | ((self.g as u32) << 8)
            | (self.b as u32)
    }
}

/// Information describing linear framebuffer configuration.
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    pub addr: *mut u8,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub bpp: usize,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
}

unsafe impl Send for FramebufferInfo {}
unsafe impl Sync for FramebufferInfo {}

/// Linear Framebuffer device abstraction.
pub struct Framebuffer {
    pub info: FramebufferInfo,
}

unsafe impl Send for Framebuffer {}
unsafe impl Sync for Framebuffer {}

impl Framebuffer {
    /// Creates a new Framebuffer instance from hardware/bootloader info.
    pub fn new(info: FramebufferInfo) -> Self {
        Self { info }
    }

    #[inline(always)]
    pub fn info(&self) -> &FramebufferInfo {
        &self.info
    }

    #[inline(always)]
    pub fn width(&self) -> usize {
        self.info.width
    }

    #[inline(always)]
    pub fn height(&self) -> usize {
        self.info.height
    }

    #[inline(always)]
    pub fn pitch(&self) -> usize {
        self.info.pitch
    }

    #[inline(always)]
    pub fn bpp(&self) -> usize {
        self.info.bpp
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.info.pitch * self.info.height
    }

    /// Converts a color into the hardware pixel value matching RGB bitmask shifts.
    #[inline(always)]
    pub fn encode_pixel(&self, color: Color) -> u32 {
        let r = ((color.r as u32) & ((1 << self.info.red_mask_size) - 1))
            << self.info.red_mask_shift;
        let g = ((color.g as u32) & ((1 << self.info.green_mask_size) - 1))
            << self.info.green_mask_shift;
        let b = ((color.b as u32) & ((1 << self.info.blue_mask_size) - 1))
            << self.info.blue_mask_shift;
        r | g | b
    }

    /// Plots a single pixel at (x, y) with boundary checking.
    #[inline(always)]
    pub fn put_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }

        let pixel_value = self.encode_pixel(color);
        let bytes_per_pixel = self.info.bpp / 8;
        let offset = y * self.info.pitch + x * bytes_per_pixel;

        // SAFETY: Bounds checked against width and height, pointer points to mapped framebuffer memory.
        unsafe {
            let pixel_ptr = self.info.addr.add(offset);
            match bytes_per_pixel {
                4 => ptr::write_volatile(pixel_ptr as *mut u32, pixel_value),
                3 => {
                    *pixel_ptr = (pixel_value & 0xFF) as u8;
                    *pixel_ptr.add(1) = ((pixel_value >> 8) & 0xFF) as u8;
                    *pixel_ptr.add(2) = ((pixel_value >> 16) & 0xFF) as u8;
                }
                2 => ptr::write_volatile(pixel_ptr as *mut u16, pixel_value as u16),
                _ => {}
            }
        }
    }

    /// Reads a single pixel from (x, y).
    pub fn get_pixel(&self, x: usize, y: usize) -> Color {
        if x >= self.info.width || y >= self.info.height {
            return Color::BLACK;
        }

        let bytes_per_pixel = self.info.bpp / 8;
        let offset = y * self.info.pitch + x * bytes_per_pixel;

        // SAFETY: Bounds checked against width and height.
        let raw_val = unsafe {
            let pixel_ptr = self.info.addr.add(offset);
            match bytes_per_pixel {
                4 => ptr::read_volatile(pixel_ptr as *const u32),
                2 => ptr::read_volatile(pixel_ptr as *const u16) as u32,
                _ => 0,
            }
        };

        let r = ((raw_val >> self.info.red_mask_shift) & ((1 << self.info.red_mask_size) - 1)) as u8;
        let g = ((raw_val >> self.info.green_mask_shift) & ((1 << self.info.green_mask_size) - 1)) as u8;
        let b = ((raw_val >> self.info.blue_mask_shift) & ((1 << self.info.blue_mask_size) - 1)) as u8;
        Color::rgb(r, g, b)
    }

    /// Clears the entire screen with a specified background color.
    pub fn clear(&mut self, color: Color) {
        self.fill_rect(0, 0, self.info.width, self.info.height, color);
    }

    /// Fills a rectangular region with the specified color.
    pub fn fill_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: Color) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }

        let x_end = core::cmp::min(x + width, self.info.width);
        let y_end = core::cmp::min(y + height, self.info.height);
        let pixel_value = self.encode_pixel(color);
        let bytes_per_pixel = self.info.bpp / 8;

        for cur_y in y..y_end {
            let row_offset = cur_y * self.info.pitch;
            for cur_x in x..x_end {
                let offset = row_offset + cur_x * bytes_per_pixel;
                // SAFETY: Offsets are within valid bounds of the framebuffer.
                unsafe {
                    let pixel_ptr = self.info.addr.add(offset);
                    if bytes_per_pixel == 4 {
                        ptr::write_volatile(pixel_ptr as *mut u32, pixel_value);
                    } else if bytes_per_pixel == 3 {
                        *pixel_ptr = (pixel_value & 0xFF) as u8;
                        *pixel_ptr.add(1) = ((pixel_value >> 8) & 0xFF) as u8;
                        *pixel_ptr.add(2) = ((pixel_value >> 16) & 0xFF) as u8;
                    } else if bytes_per_pixel == 2 {
                        ptr::write_volatile(pixel_ptr as *mut u16, pixel_value as u16);
                    }
                }
            }
        }
    }

    /// Draws an unfilled rectangle boundary.
    pub fn draw_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: Color) {
        if width == 0 || height == 0 {
            return;
        }
        self.draw_line(x, y, x + width - 1, y, color);
        self.draw_line(x, y + height - 1, x + width - 1, y + height - 1, color);
        self.draw_line(x, y, x, y + height - 1, color);
        self.draw_line(x + width - 1, y, x + width - 1, y + height - 1, color);
    }

    /// Draws a line using Bresenham's line algorithm.
    pub fn draw_line(&mut self, x0: usize, y0: usize, x1: usize, y1: usize, color: Color) {
        let mut x0 = x0 as isize;
        let mut y0 = y0 as isize;
        let x1 = x1 as isize;
        let y1 = y1 as isize;

        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            if x0 >= 0 && y0 >= 0 {
                self.put_pixel(x0 as usize, y0 as usize, color);
            }

            if x0 == x1 && y0 == y1 {
                break;
            }

            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    /// Renders a single character glyph at pixel coordinates (x, y).
    pub fn draw_char(&mut self, x: usize, y: usize, c: char, fg: Color, bg: Color) {
        if x + FONT_WIDTH > self.info.width || y + FONT_HEIGHT > self.info.height {
            return;
        }

        let glyph = get_glyph(c);
        let fg_val = self.encode_pixel(fg);
        let bg_val = self.encode_pixel(bg);
        let bytes_per_pixel = self.info.bpp / 8;

        for (row_idx, &row_byte) in glyph.iter().enumerate() {
            let row_offset = (y + row_idx) * self.info.pitch;
            for col_idx in 0..FONT_WIDTH {
                let is_fg = (row_byte & (0x80 >> col_idx)) != 0;
                let pixel_val = if is_fg { fg_val } else { bg_val };
                let offset = row_offset + (x + col_idx) * bytes_per_pixel;

                // SAFETY: x and y within framebuffer dimensions.
                unsafe {
                    let pixel_ptr = self.info.addr.add(offset);
                    if bytes_per_pixel == 4 {
                        ptr::write_volatile(pixel_ptr as *mut u32, pixel_val);
                    } else if bytes_per_pixel == 3 {
                        *pixel_ptr = (pixel_val & 0xFF) as u8;
                        *pixel_ptr.add(1) = ((pixel_val >> 8) & 0xFF) as u8;
                        *pixel_ptr.add(2) = ((pixel_val >> 16) & 0xFF) as u8;
                    } else if bytes_per_pixel == 2 {
                        ptr::write_volatile(pixel_ptr as *mut u16, pixel_val as u16);
                    }
                }
            }
        }
    }

    /// Renders a string of text at pixel coordinates (x, y).
    pub fn draw_string(&mut self, x: usize, y: usize, s: &str, fg: Color, bg: Color) {
        let mut cur_x = x;
        let mut cur_y = y;

        for c in s.chars() {
            if c == '\n' {
                cur_x = x;
                cur_y += FONT_HEIGHT;
                continue;
            }
            if cur_x + FONT_WIDTH > self.info.width {
                cur_x = x;
                cur_y += FONT_HEIGHT;
            }
            if cur_y + FONT_HEIGHT > self.info.height {
                break;
            }
            self.draw_char(cur_x, cur_y, c, fg, bg);
            cur_x += FONT_WIDTH;
        }
    }

    /// Scrolls the screen up by `pixels` rows, filling the newly exposed bottom rows with `bg`.
    pub fn scroll_up(&mut self, pixels: usize, bg: Color) {
        if pixels >= self.info.height {
            self.clear(bg);
            return;
        }

        let bytes_to_copy = (self.info.height - pixels) * self.info.pitch;
        let src_offset = pixels * self.info.pitch;

        // SAFETY: Source and destination buffers are within the mapped framebuffer memory.
        unsafe {
            ptr::copy(
                self.info.addr.add(src_offset),
                self.info.addr,
                bytes_to_copy,
            );
        }

        // Fill remaining bottom region with background color
        self.fill_rect(
            0,
            self.info.height - pixels,
            self.info.width,
            pixels,
            bg,
        );
    }
}
