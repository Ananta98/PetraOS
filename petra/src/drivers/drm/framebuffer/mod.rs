// SPDX-License-Identifier: MPL-2.0

//! Framebuffer Subsystem.
//!
//! Based on Asterinas framebuffer and console architecture.

pub mod framebuffer;
pub mod mode;
pub mod pixel;

pub use framebuffer::{ColorMapEntry, FrameBuffer, PixelOffset, get_framebuffer};
pub use mode::{ConsoleMode, KeyboardMode, KeyboardModeFlags};
pub use pixel::{Color, Pixel, PixelFormat, RenderedPixel};

use crate::uapi::errno::Result;

pub fn init() -> Result<()> {
    match framebuffer::init() {
        Ok(()) => Ok(()),
        Err(crate::uapi::errno::Errno::ENODEV) => {
            ostd::info!("DRM Framebuffer Subsystem: No boot framebuffer present");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

crate::module!(
    "DRM Framebuffer Subsystem",
    "Ananta98",
    crate::modules::InitcallLevel::Device,
    init
);
