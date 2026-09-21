//! Character Device Drivers

pub mod keyboard;
pub mod mouse;
pub mod serial;

pub use keyboard::{Ps2Keyboard, handle_scancode, read_char, interrupt_count as keyboard_interrupt_count};
pub use mouse::{Ps2Mouse, handle_mouse_byte, interrupt_count as mouse_interrupt_count};
