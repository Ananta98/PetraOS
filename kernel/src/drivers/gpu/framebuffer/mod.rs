pub mod color;
pub mod draw;
pub mod font;

pub use color::{Color, PixelFormat, VideoMode};
pub use draw::Framebuffer;

use crate::drivers::gpu::{GPU_MANAGER, GpuDriver};
use alloc::sync::Arc;
use spin::Once;

impl GpuDriver for Framebuffer {
    fn name(&self) -> &str {
        "framebuffer"
    }

    fn current_mode(&self) -> VideoMode {
        self.mode()
    }

    fn set_mode(&self, mode: VideoMode) -> Result<(), ostd::Error> {
        if mode == self.mode() {
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

        // 4. Test draw_char with font8x8
        fb.draw_char(0, 0, 'A', red);
        {
            let pixels = fb.pixels.lock();
            // 'A' top row in font8x8 is 0x3C = 0b00111100
            // bits 2,3,4,5 are set (column 2, 3, 4, 5)
            let offset = 2 * 4; // x=2, y=0
            assert_eq!(pixels[offset], red.r);
            assert_eq!(pixels[offset + 1], red.g);
            assert_eq!(pixels[offset + 2], red.b);
            assert_eq!(pixels[offset + 3], red.a);
        }
    }
}
