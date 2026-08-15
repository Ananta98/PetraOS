//! Limine GPU & Console Framebuffer Driver
//!
//! Handles Limine bootloader framebuffer acquisition, initialization,
//! text console activation, and DEVICE_MANAGER registration.

pub mod console;
pub mod device;
pub mod fb;
pub mod font;

use alloc::boxed::Box;
use alloc::sync::Arc;
use crate::device::{Device, Driver, DriverError};
use crate::sync::spinlock::Spinlock;

pub use console::{fb_console_get_dimensions, fb_console_write_byte, fb_console_write_str, FbConsole, FB_CONSOLE};
pub use device::{FramebufferConsoleDevice, FramebufferDevice};
pub use fb::{Color, Framebuffer, FramebufferInfo, FRAMEBUFFER};
pub use font::{get_glyph, FONT_HEIGHT, FONT_WIDTH};

/// Driver structure for Limine Framebuffer.
#[derive(Default)]
pub struct FramebufferDriver;

impl Driver for FramebufferDriver {
    fn name(&self) -> &'static str {
        "framebuffer"
    }

    fn bus_name(&self) -> &'static str {
        "platform"
    }

    fn description(&self) -> &'static str {
        "Limine GPU & Console Framebuffer Driver"
    }

    fn probe(&self) -> Result<(), DriverError> {
        let response = match crate::limine::FRAMEBUFFER_REQUEST.get_response() {
            Some(resp) => resp,
            None => {
                log::warn!("[Framebuffer] No Limine framebuffer response found.");
                return Err(DriverError::Unsupported);
            }
        };

        let limine_fb = match response.framebuffers().next() {
            Some(fb) => fb,
            None => {
                log::warn!("[Framebuffer] Framebuffer list is empty.");
                return Err(DriverError::Unsupported);
            }
        };

        let info = FramebufferInfo {
            addr: limine_fb.addr(),
            width: limine_fb.width() as usize,
            height: limine_fb.height() as usize,
            pitch: limine_fb.pitch() as usize,
            bpp: limine_fb.bpp() as usize,
            red_mask_size: limine_fb.red_mask_size(),
            red_mask_shift: limine_fb.red_mask_shift(),
            green_mask_size: limine_fb.green_mask_size(),
            green_mask_shift: limine_fb.green_mask_shift(),
            blue_mask_size: limine_fb.blue_mask_size(),
            blue_mask_shift: limine_fb.blue_mask_shift(),
        };

        log::info!(
            "[Framebuffer] Detected: {}x{} @ {}bpp, pitch={}, addr={:p}",
            info.width,
            info.height,
            info.bpp,
            info.pitch,
            info.addr
        );

        let mut fb = Framebuffer::new(info);
        fb.clear(Color::rgb(15, 15, 20));
        *FRAMEBUFFER.lock() = Some(fb);

        // Initialize and activate the text console
        let mut console = FbConsole::new(info.width, info.height);
        console.clear();
        console.write_str("\x1B[32m[PetraOS]\x1B[0m Framebuffer Console Initialized.\n");
        *FB_CONSOLE.lock() = Some(console);

        // Register FramebufferDevice (GPU) to DEVICE_MANAGER
        let fb_dev_ref: Arc<Spinlock<Box<dyn Device>>> =
            Arc::new(Spinlock::new(Box::new(FramebufferDevice)));
        crate::device::DEVICE_MANAGER.write().register(fb_dev_ref);

        // Register FramebufferConsoleDevice (Char) to DEVICE_MANAGER
        let con_dev_ref: Arc<Spinlock<Box<dyn Device>>> =
            Arc::new(Spinlock::new(Box::new(FramebufferConsoleDevice)));
        crate::device::DEVICE_MANAGER.write().register(con_dev_ref);

        log::info!("[Framebuffer] GPU and Console Framebuffer registered successfully.");
        Ok(())
    }
}

/// Explicit initialization entry point for the framebuffer driver.
pub fn init() -> Result<(), DriverError> {
    FramebufferDriver::default().probe()
}

crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("PetraOS Development Team");
crate::MODULE_DESCRIPTION!("Limine GPU & Console Framebuffer Driver");
crate::MODULE_VERSION!("1.0.0");
crate::module_driver!(
    FRAMEBUFFER_INITCALL,
    framebuffer_driver_init,
    "framebuffer",
    FramebufferDriver
);
