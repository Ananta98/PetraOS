//! Interface Segregation Principle (ISP): granular UI traits.

use space_core::{Canvas, Point, Rect};

#[derive(Debug, Clone, Copy)]
pub enum UiEvent {
    MouseMove { pos: Point, buttons: i32 },
    MouseDown { pos: Point, button: i32 },
    MouseUp { pos: Point, button: i32 },
    Wheel { pos: Point, delta: i32 },
    Key { code: u16, pressed: bool, ch: Option<char>, mods: i32 },
    Resize { width: u32, height: u32 },
    Focus(bool),
}

/// Rendering contract for any drawable UI element.
pub trait Paintable {
    fn paint(&self, canvas: &mut dyn Canvas);
}

/// Input handling contract for interactive elements.
pub trait EventListener {
    /// Returns `true` if the event was handled and requires a redraw.
    fn on_event(&mut self, event: UiEvent) -> bool;
}

/// Geometry/layout contract.
pub trait Layoutable {
    fn bounds(&self) -> Rect;
    fn set_bounds(&mut self, rect: Rect);
}

/// Lifecycle contract for application surfaces and widgets.
pub trait Lifecycle {
    fn on_init(&mut self) {}
    fn on_tick(&mut self) -> bool {
        false
    }
    fn on_close(&mut self) {}
}
