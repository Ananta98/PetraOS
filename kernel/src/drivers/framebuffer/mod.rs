use crate::limine::FRAMEBUFFER_REQUEST;
use crate::sync::spinlock::Spinlock;

pub struct Framebuffer {
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub address: *mut u8,
}

unsafe impl Send for Framebuffer {}
unsafe impl Sync for Framebuffer {}

impl Framebuffer {
    pub fn new() -> Option<Self> {
        if let Some(response) = FRAMEBUFFER_REQUEST.get_response() {
            if let Some(fb) = response.framebuffers().next() {
                return Some(Framebuffer {
                    width: fb.width(),
                    height: fb.height(),
                    pitch: fb.pitch(),
                    bpp: fb.bpp(),
                    address: fb.addr(),
                });
            }
        }
        None
    }

    pub fn clear(&mut self, color: u32) {
        if color == 0 {
            unsafe {
                let size = (self.height * self.pitch) as usize;
                core::ptr::write_bytes(self.address, 0, size);
            }
        } else {
            for y in 0..self.height {
                for x in 0..self.width {
                    self.put_pixel(x, y, color);
                }
            }
        }
    }

    #[inline(always)]
    pub fn put_pixel(&mut self, x: u64, y: u64, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }

        let offset = y * self.pitch + x * (self.bpp as u64 / 8);
        unsafe {
            let ptr = self.address.add(offset as usize) as *mut u32;
            core::ptr::write_volatile(ptr, color);
        }
    }

    pub fn draw_rect(&mut self, x: u64, y: u64, width: u64, height: u64, color: u32) {
        for dy in 0..height {
            for dx in 0..width {
                self.put_pixel(x + dx, y + dy, color);
            }
        }
    }

    pub fn draw_line(&mut self, mut x0: i64, mut y0: i64, x1: i64, y1: i64, color: u32) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            if x0 >= 0 && y0 >= 0 {
                self.put_pixel(x0 as u64, y0 as u64, color);
            }
            if x0 == x1 && y0 == y1 { break; }
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
}

pub static FB: Spinlock<Option<Framebuffer>> = Spinlock::new(None);

pub fn init() {
    if let Some(mut fb) = Framebuffer::new() {
        fb.clear(0);
        *FB.lock() = Some(fb);
    }
}
