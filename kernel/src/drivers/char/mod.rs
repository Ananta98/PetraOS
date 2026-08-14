//! Character Device Drivers

pub mod keyboard;
pub mod serial;

pub use keyboard::{Ps2Keyboard, handle_scancode, read_char, interrupt_count};
