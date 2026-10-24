// SPDX-License-Identifier: GPL-2.0

//! Virtual Terminal (VT) Subsystem.
//!
//! Handles terminal emulation (VT100/XTerm escape sequences), framebuffer font
//! rendering, and keyboard event dispatching.

pub mod font;
pub mod kbd;
pub mod terminal;

pub use font::ConsoleFont;
pub use terminal::{FramebufferDisplay, get_winsize, write_output};
