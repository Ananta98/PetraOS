// SPDX-License-Identifier: GPL-2.0

//! Framebuffer Subsystem.
//!
//! Provides a Linux fbdev / FreeBSD vt inspired graphics and virtual terminal
//! subsystem, including `/dev/fb0` character device integration, 8x16 bitmap
//! font rendering, ANSI escape sequences, and physical display management.

pub mod ansi;
pub mod console;
pub mod dev;
pub mod fb;
pub mod font;
pub mod pixel;

pub use console::FbConsole;
pub use dev::FbDev;
pub use fb::Framebuffer;
pub use font::{FONT_HEIGHT, FONT_WIDTH};
pub use pixel::{Color, PixelFormat};

use alloc::sync::Arc;
use spin::{Mutex, Once};

use crate::api::errno::Result;

static FRAMEBUFFER: Once<Arc<Framebuffer>> = Once::new();
static CONSOLE: Once<Mutex<FbConsole>> = Once::new();
static FB_DEV: Once<Arc<FbDev>> = Once::new();

/// Initializes the framebuffer subsystem and registers the console and `/dev/fb0`.
pub fn init() -> Result<()> {
    ostd::info!("Initializing Framebuffer subsystem...");

    let fb = Arc::new(Framebuffer::probe()?);
    let console = Mutex::new(FbConsole::new(Arc::clone(&fb)));
    let fb_dev = Arc::new(FbDev::new(Arc::clone(&fb)));

    // Register /dev/fb0 in the global character device registry
    crate::drivers::register_chrdev(Arc::clone(&fb_dev) as Arc<dyn crate::drivers::CharacterDevice>);

    // Store singletons
    let _ = FRAMEBUFFER.call_once(|| Arc::clone(&fb));
    let _ = CONSOLE.call_once(|| console);
    let _ = FB_DEV.call_once(|| fb_dev);

    ostd::info!("Framebuffer: Console and /dev/fb0 successfully initialized");
    Ok(())
}

/// Returns a reference to the global virtual terminal console, if initialized.
pub fn get_console() -> Option<&'static Mutex<FbConsole>> {
    CONSOLE.get()
}

/// Returns a reference to the underlying linear framebuffer, if initialized.
pub fn get_framebuffer() -> Option<Arc<Framebuffer>> {
    FRAMEBUFFER.get().cloned()
}

// ----------------------------------------------------------------------------
// Kernel Module Declaration via module! macro
// ----------------------------------------------------------------------------

crate::module!(
    "DRM Framebuffer Subsystem",
    "Ananta98",
    crate::modules::InitcallLevel::Device,
    init
);
