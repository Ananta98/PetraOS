pub mod color;
pub mod console;
pub mod draw;
pub mod fbdev;
pub mod font;

pub use color::{Color, PixelFormat, VideoMode};
pub use console::{FbConsole, FbConsoleDriver, fb_console};
pub use draw::Framebuffer;
pub use fbdev::FbDev;

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

pub struct FramebufferDriver;

impl crate::device::Driver for FramebufferDriver {
    fn name(&self) -> &str {
        "framebuffer"
    }

    fn bus_name(&self) -> &str {
        "virtual"
    }

    fn description(&self) -> &str {
        "VESA/VGA Framebuffer Graphics Display Driver"
    }

    fn probe(&self) -> Result<(), ostd::Error> {
        let boot_fb = ostd::boot::boot_info().framebuffer_arg;
        let (mode, io_mem_opt) = if let Some(fb_arg) = boot_fb {
            let paddr = fb_arg.address;
            let bpp_bytes = ((fb_arg.bpp as usize) + 7) / 8;
            let pitch = (fb_arg.width as usize) * bpp_bytes;
            let size = (fb_arg.height as usize) * pitch;

            ostd::early_println!(
                "[framebuffer] GOP Boot Info: physical_address={:#x}, width={}, height={}, bpp={}, pitch={}",
                paddr,
                fb_arg.width,
                fb_arg.height,
                fb_arg.bpp,
                pitch
            );

            let io_mem = match ostd::io::IoMem::acquire(paddr..paddr + size) {
                Ok(mem) => {
                    ostd::early_println!(
                        "[framebuffer] Successfully mapped MMIO physical {:#x}..{:#x} with IoMem",
                        paddr,
                        paddr + size
                    );
                    Some(mem)
                }
                Err(err) => {
                    ostd::early_println!(
                        "[framebuffer] WARNING: Failed to acquire IoMem for physical {:#x}: {:?}",
                        paddr,
                        err
                    );
                    None
                }
            };

            let mode = VideoMode {
                width: fb_arg.width as u32,
                height: fb_arg.height as u32,
                pitch: pitch as u32,
                bpp: fb_arg.bpp as u32,
                format: PixelFormat::Rgba8888,
            };

            (mode, io_mem)
        } else {
            ostd::early_println!(
                "[framebuffer] WARNING: GOP Framebuffer boot_info is None! Initializing fallback 1024x768 framebuffer."
            );
            let mode = VideoMode {
                width: 1024,
                height: 768,
                pitch: 1024 * 4,
                bpp: 32,
                format: PixelFormat::Rgba8888,
            };
            (mode, None)
        };

        let fb = if let Some(io_mem) = io_mem_opt {
            Arc::new(Framebuffer::new_mmio(mode, io_mem))
        } else {
            Arc::new(Framebuffer::new(mode))
        };

        FRAMEBUFFER.call_once(|| fb.clone());
        let _ = GPU_MANAGER.register_driver(fb.clone());

        let fbdev = Arc::new(fbdev::FbDev::new(fb));
        let _ = crate::drivers::char::register_char_device("fb0", fbdev);
        Ok(())
    }
}

crate::module_driver!(
    FRAMEBUFFER_INITCALL,
    framebuffer_driver_init,
    "framebuffer",
    FramebufferDriver
);

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
