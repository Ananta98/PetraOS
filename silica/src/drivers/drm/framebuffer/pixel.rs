// SPDX-License-Identifier: MPL-2.0

//! Pixel types and framebuffer formats.
//!
//! Based on Asterinas framebuffer pixel module.

/// Individual pixel data containing raw RGB channel values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pixel {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

pub type Color = Pixel;

/// Pixel format that defines the memory layout of each pixel in the framebuffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelFormat {
    /// Each pixel uses 8 bits to represent its grayscale intensity, ranging from 0 (black) to 255 (white).
    Grayscale8,
    /// Each pixel uses 16 bits, with 5 bits for Red, 6 bits for Green, and 5 bits for Blue.
    Rgb565,
    /// Each pixel uses 24 bits, with 8 bits for Red, 8 bits for Green, and 8 bits for Blue.
    Rgb888,
    /// Each pixel uses 32 bits, with 8 bits for Blue, 8 bits for Green, 8 bits for Red, and 8 bits reserved.
    BgrReserved,
}

/// A rendered pixel in a specific format ready for blitting.
#[derive(Clone, Copy, Debug)]
pub struct RenderedPixel {
    buf: [u8; 4],
    len: u8,
}

impl Pixel {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    pub const WHITE: Pixel = Pixel {
        red: 0xFF,
        green: 0xFF,
        blue: 0xFF,
    };
    pub const BLACK: Pixel = Pixel {
        red: 0x00,
        green: 0x00,
        blue: 0x00,
    };
    pub const RED: Pixel = Pixel {
        red: 0xAA,
        green: 0x00,
        blue: 0x00,
    };
    pub const GREEN: Pixel = Pixel {
        red: 0x00,
        green: 0xAA,
        blue: 0x00,
    };
    pub const YELLOW: Pixel = Pixel {
        red: 0xAA,
        green: 0x55,
        blue: 0x00,
    };
    pub const BLUE: Pixel = Pixel {
        red: 0x00,
        green: 0x00,
        blue: 0xAA,
    };
    pub const MAGENTA: Pixel = Pixel {
        red: 0xAA,
        green: 0x00,
        blue: 0xAA,
    };
    pub const CYAN: Pixel = Pixel {
        red: 0x00,
        green: 0xAA,
        blue: 0xAA,
    };

    pub const BRIGHT_BLACK: Pixel = Pixel {
        red: 0x55,
        green: 0x55,
        blue: 0x55,
    };
    pub const BRIGHT_RED: Pixel = Pixel {
        red: 0xFF,
        green: 0x55,
        blue: 0x55,
    };
    pub const BRIGHT_GREEN: Pixel = Pixel {
        red: 0x55,
        green: 0xFF,
        blue: 0x55,
    };
    pub const BRIGHT_YELLOW: Pixel = Pixel {
        red: 0xFF,
        green: 0xFF,
        blue: 0x55,
    };
    pub const BRIGHT_BLUE: Pixel = Pixel {
        red: 0x55,
        green: 0x55,
        blue: 0xFF,
    };
    pub const BRIGHT_MAGENTA: Pixel = Pixel {
        red: 0xFF,
        green: 0x55,
        blue: 0xFF,
    };
    pub const BRIGHT_CYAN: Pixel = Pixel {
        red: 0x55,
        green: 0xFF,
        blue: 0xFF,
    };
    pub const BRIGHT_WHITE: Pixel = Pixel {
        red: 0xFF,
        green: 0xFF,
        blue: 0xFF,
    };

    /// Converts standard ANSI 3-bit / 4-bit color index (0..15) to a `Pixel`.
    pub const fn from_ansi(code: u8) -> Self {
        match code {
            0 => Self::BLACK,
            1 => Self::RED,
            2 => Self::GREEN,
            3 => Self::YELLOW,
            4 => Self::BLUE,
            5 => Self::MAGENTA,
            6 => Self::CYAN,
            7 => Self::WHITE,
            8 => Self::BRIGHT_BLACK,
            9 => Self::BRIGHT_RED,
            10 => Self::BRIGHT_GREEN,
            11 => Self::BRIGHT_YELLOW,
            12 => Self::BRIGHT_BLUE,
            13 => Self::BRIGHT_MAGENTA,
            14 => Self::BRIGHT_CYAN,
            15 => Self::BRIGHT_WHITE,
            _ => Self::WHITE,
        }
    }

    /// Backwards-compatible RGB accessors.
    pub const fn r(&self) -> u8 {
        self.red
    }
    pub const fn g(&self) -> u8 {
        self.green
    }
    pub const fn b(&self) -> u8 {
        self.blue
    }

    /// Renders the pixel into a specific format.
    pub fn render(&self, format: PixelFormat) -> RenderedPixel {
        let mut buf = [0; 4];
        match format {
            PixelFormat::Grayscale8 => {
                let red_weight = 77 * self.red as u16;
                let green_weight = 150 * self.green as u16;
                let blue_weight = 29 * self.blue as u16;
                let grayscale = (red_weight + green_weight + blue_weight) >> 8;
                buf[0] = grayscale as u8;
                RenderedPixel { buf, len: 1 }
            }
            PixelFormat::Rgb565 => {
                let r = (self.red >> 3) as u16;
                let g = (self.green >> 2) as u16;
                let b = (self.blue >> 3) as u16;
                let rgb565 = (r << 11) | (g << 5) | b;
                buf[0..2].copy_from_slice(&rgb565.to_be_bytes());
                RenderedPixel { buf, len: 2 }
            }
            PixelFormat::Rgb888 => {
                buf[0] = self.red;
                buf[1] = self.green;
                buf[2] = self.blue;
                RenderedPixel { buf, len: 3 }
            }
            PixelFormat::BgrReserved => {
                buf[0] = self.blue;
                buf[1] = self.green;
                buf[2] = self.red;
                buf[3] = 0;
                RenderedPixel { buf, len: 4 }
            }
        }
    }
}

impl PixelFormat {
    /// Returns the number of bytes per pixel (color depth).
    pub fn nbytes(&self) -> usize {
        match self {
            PixelFormat::Grayscale8 => 1,
            PixelFormat::Rgb565 => 2,
            PixelFormat::Rgb888 => 3,
            PixelFormat::BgrReserved => 4,
        }
    }

    pub fn bytes_per_pixel(&self) -> usize {
        self.nbytes()
    }
}

impl RenderedPixel {
    /// Returns the number of bytes in the rendered pixel.
    pub fn nbytes(&self) -> usize {
        self.len as usize
    }

    /// Returns a slice to the rendered pixel data.
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.nbytes()]
    }
}
