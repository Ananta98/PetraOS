// SPDX-License-Identifier: MPL-2.0

//! Framebuffer Subsystem.
//!
//! Based on Asterinas framebuffer and console architecture.

pub mod ansi;
pub mod console;
pub mod font;
pub mod framebuffer;
pub mod mode;
pub mod pixel;

pub use console::{get_console, FramebufferConsole};
pub use font::{BitmapFont, FONT_HEIGHT, FONT_WIDTH};
pub use framebuffer::{get_framebuffer, ColorMapEntry, FrameBuffer, Framebuffer, PixelOffset};
pub use mode::{ConsoleMode, KeyboardMode, KeyboardModeFlags};
pub use pixel::{Color, Pixel, PixelFormat, RenderedPixel};

use crate::api::errno::Result;

pub fn init() -> Result<()> {
    framebuffer::init()?;
    Ok(())
}

crate::module!(
    "DRM Framebuffer Subsystem",
    "Ananta98",
    crate::modules::InitcallLevel::Device,
    init
);
