// SPDX-License-Identifier: GPL-2.0

//! System Console TTY Driver.
//!
//! Connects the primary system console (`/dev/console`) to the DRM framebuffer console.

use crate::api::errno::Result;
use crate::api::termios::winsize;
use super::{Tty, TtyDriver};

/// Driver delivering TTY output directly to the Framebuffer Console.
pub struct FramebufferConsoleDriver;

impl TtyDriver for FramebufferConsoleDriver {
    fn write_output(&self, data: &[u8]) -> Result<()> {
        if let Some(cons_lock) = crate::drivers::drm::get_console() {
            let mut cons = cons_lock.lock();
            cons.send(data);
        }
        Ok(())
    }

    fn get_winsize(&self) -> winsize {
        if let Some(cons_lock) = crate::drivers::drm::get_console() {
            let cons = cons_lock.lock();
            let fb = crate::drivers::drm::get_framebuffer();
            let (w, h) = fb.map(|f| (f.width as u16, f.height as u16)).unwrap_or((1280, 800));
            winsize {
                ws_row: cons.rows() as u16,
                ws_col: cons.cols() as u16,
                ws_xpixel: w,
                ws_ypixel: h,
            }
        } else {
            winsize {
                ws_row: 25,
                ws_col: 80,
                ws_xpixel: 1280,
                ws_ypixel: 800,
            }
        }
    }
}

pub type Console = Tty;
