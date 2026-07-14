use crate::drivers::{Device, DeviceType, register_device};
use crate::fs::vfs::{DirEntry, FileOps, FileType, InodeOps, Metadata, SeekFrom};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::sync::SpinLock;
use spin::Once;

/// Framebuffer screen width in pixels.
pub const FB_WIDTH: usize = 1024;
/// Framebuffer screen height in pixels.
pub const FB_HEIGHT: usize = 768;
/// Bytes per pixel (32-bit RGBA).
pub const FB_BPP: usize = 4;
/// Total size of the framebuffer in bytes.
pub const FB_SIZE: usize = FB_WIDTH * FB_HEIGHT * FB_BPP;

/// Represents a 32-bit RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };
    pub const RED: Self = Self { r: 255, g: 0, b: 0, a: 255 };
    pub const GREEN: Self = Self { r: 0, g: 255, b: 0, a: 255 };
    pub const BLUE: Self = Self { r: 0, g: 0, b: 255, a: 255 };
}

/// The global framebuffer driver instance.
pub struct Framebuffer {
    /// Lock-protected pixel buffer.
    pub pixels: SpinLock<Vec<u8>>,
}

impl Framebuffer {
    /// Create a new Framebuffer initialized to black.
    pub fn new() -> Self {
        Self {
            pixels: SpinLock::new(alloc::vec![0u8; FB_SIZE]),
        }
    }

    /// Clear the screen with a specific color.
    pub fn clear(&self, color: Color) {
        let mut p = self.pixels.lock();
        for pixel in p.chunks_exact_mut(FB_BPP) {
            pixel[0] = color.r;
            pixel[1] = color.g;
            pixel[2] = color.b;
            pixel[3] = color.a;
        }
    }

    /// Draw a single pixel at (x, y) with a color.
    pub fn draw_pixel(&self, x: usize, y: usize, color: Color) {
        if x >= FB_WIDTH || y >= FB_HEIGHT {
            return;
        }
        let offset = (y * FB_WIDTH + x) * FB_BPP;
        let mut p = self.pixels.lock();
        p[offset] = color.r;
        p[offset + 1] = color.g;
        p[offset + 2] = color.b;
        p[offset + 3] = color.a;
    }

    /// Draw a solid rectangle.
    pub fn draw_rect(&self, x: usize, y: usize, w: usize, h: usize, color: Color) {
        let mut p = self.pixels.lock();
        for row in y..core::cmp::min(y + h, FB_HEIGHT) {
            for col in x..core::cmp::min(x + w, FB_WIDTH) {
                let offset = (row * FB_WIDTH + col) * FB_BPP;
                p[offset] = color.r;
                p[offset + 1] = color.g;
                p[offset + 2] = color.b;
                p[offset + 3] = color.a;
            }
        }
    }

    /// Draw an ASCII character on the screen using a simple built-in 8x8 font.
    pub fn draw_char(&self, x: usize, y: usize, ch: char, color: Color) {
        let c = ch as usize;
        if c < 32 || c > 126 {
            return; // unsupported character
        }
        let font_idx = c - 32;
        let bitmap = SIMPLE_FONT[font_idx];
        for row in 0..8 {
            let row_byte = bitmap[row];
            for col in 0..8 {
                if (row_byte & (1 << (7 - col))) != 0 {
                    self.draw_pixel(x + col, y + row, color);
                }
            }
        }
    }

    /// Draw a text string on the screen.
    pub fn draw_string(&self, x: usize, y: usize, s: &str, color: Color) {
        let mut curr_x = x;
        for ch in s.chars() {
            if ch == '\n' {
                continue;
            }
            self.draw_char(curr_x, y, ch, color);
            curr_x += 8;
        }
    }

    /// Draw a line from (x0, y0) to (x1, y1) with a color.
    pub fn draw_line(&self, x0: isize, y0: isize, x1: isize, y1: isize, color: Color) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        let mut x = x0;
        let mut y = y0;

        loop {
            if x >= 0 && x < FB_WIDTH as isize && y >= 0 && y < FB_HEIGHT as isize {
                self.draw_pixel(x as usize, y as usize, color);
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
    pub fn draw_circle(&self, xc: isize, yc: isize, r: isize, color: Color) {
        let mut x = 0;
        let mut y = r;
        let mut d = 3 - 2 * r;

        let draw_symmetric = |x_val: isize, y_val: isize| {
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
                if px >= 0 && px < FB_WIDTH as isize && py >= 0 && py < FB_HEIGHT as isize {
                    self.draw_pixel(px as usize, py as usize, color);
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
}

// ---------------------------------------------------------------------------
// VFS / Device Glue
// ---------------------------------------------------------------------------

struct FbDevice {
    name: String,
}

impl Device for FbDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Char
    }

    fn inode_ops(&self) -> Option<Arc<dyn InodeOps>> {
        Some(Arc::new(FbInode))
    }
}

struct FbInode;

impl InodeOps for FbInode {
    fn lookup(&self, _name: &str) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn create(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn mkdir(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn metadata(&self) -> Result<Metadata, ostd::Error> {
        Ok(Metadata {
            size: FB_SIZE,
            file_type: FileType::CharDevice,
            mode: 0o660,
            inode_num: 0,
            nlink: 1,
        })
    }

    fn read_link(&self) -> Result<String, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>, ostd::Error> {
        Ok(Box::new(FbFile))
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn unlink(&self, _name: &str) -> Result<(), ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &Arc<dyn InodeOps>,
        _new_name: &str,
    ) -> Result<(), ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
}

struct FbFile;

impl FileOps for FbFile {
    fn read(&mut self, buf: &mut [u8], offset: &mut usize) -> Result<usize, ostd::Error> {
        if *offset >= FB_SIZE {
            return Ok(0);
        }
        let driver = framebuffer().expect("framebuffer driver not initialized");
        let pixels = driver.pixels.lock();
        let len = core::cmp::min(buf.len(), FB_SIZE - *offset);
        buf[..len].copy_from_slice(&pixels[*offset..*offset + len]);
        *offset += len;
        Ok(len)
    }

    fn write(&mut self, buf: &[u8], offset: &mut usize) -> Result<usize, ostd::Error> {
        if *offset >= FB_SIZE {
            return Err(ostd::Error::InvalidArgs);
        }
        let driver = framebuffer().expect("framebuffer driver not initialized");
        let mut pixels = driver.pixels.lock();
        let len = core::cmp::min(buf.len(), FB_SIZE - *offset);
        pixels[*offset..*offset + len].copy_from_slice(&buf[..len]);
        *offset += len;
        Ok(len)
    }

    fn seek(&mut self, pos: SeekFrom, offset: &mut usize) -> Result<usize, ostd::Error> {
        let new_offset = match pos {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(n) => {
                let val = *offset as isize + n;
                if val < 0 {
                    return Err(ostd::Error::InvalidArgs);
                }
                val as usize
            }
            SeekFrom::End(n) => {
                let val = FB_SIZE as isize + n;
                if val < 0 {
                    return Err(ostd::Error::InvalidArgs);
                }
                val as usize
            }
        };
        *offset = core::cmp::min(new_offset, FB_SIZE);
        Ok(*offset)
    }

    fn readdir(&mut self) -> Result<Vec<DirEntry>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
}

static FB_INSTANCE: Once<Arc<Framebuffer>> = Once::new();

/// Get a reference to the global Framebuffer driver.
pub fn framebuffer() -> Option<Arc<Framebuffer>> {
    FB_INSTANCE.get().cloned()
}

/// Initialize the framebuffer device.
pub fn init() {
    let fb = Arc::new(Framebuffer::new());
    FB_INSTANCE.call_once(|| fb);

    let dev = Arc::new(FbDevice {
        name: String::from("fb0"),
    });
    let _ = register_device(dev);
}

// ---------------------------------------------------------------------------
// A Simple 8x8 font bitmap for ASCII characters 32 to 126
// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
// Unit Tests Block
// ---------------------------------------------------------------------------

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_fb_basic() {
        let fb = Framebuffer::new();
        
        // 1. Initialized to 0
        {
            let pixels = fb.pixels.lock();
            assert_eq!(pixels[0], 0);
            assert_eq!(pixels[FB_SIZE - 1], 0);
        }

        // 2. Test draw_pixel
        let red = Color::RED;
        fb.draw_pixel(10, 20, red);
        {
            let pixels = fb.pixels.lock();
            let offset = (20 * FB_WIDTH + 10) * FB_BPP;
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

        // 4. Test draw_line
        let green = Color::GREEN;
        fb.draw_line(0, 0, 10, 10, green);
        {
            let pixels = fb.pixels.lock();
            let offset = (5 * FB_WIDTH + 5) * FB_BPP;
            assert_eq!(pixels[offset], green.r);
        }

        // 5. Test draw_circle
        let white = Color::WHITE;
        fb.draw_circle(50, 50, 10, white);
        {
            let pixels = fb.pixels.lock();
            let offset = (50 * FB_WIDTH + 60) * FB_BPP;
            assert_eq!(pixels[offset], white.r);
        }
    }
}
