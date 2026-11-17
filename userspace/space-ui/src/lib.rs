pub mod client;
pub mod layout;
pub mod painter;
pub mod theme;
pub mod traits;
pub mod widgets;

pub use client::{AppLogic, SpaceClient, SurfaceCanvas};
pub use layout::{BoxLayout, Insets, SizePolicy};
pub use painter::StylePainter;
pub use theme::Theme;
pub use traits::{EventListener, Layoutable, Lifecycle, Paintable, UiEvent, WidgetState};
pub use widgets::{
    draw_panel_3d, draw_progress_bar, list_item_at, Button, CheckBox, Menu, MenuBar, MenuItem,
    Orientation, ProgressBar, ScrollBar, Splitter, StatusBar, TabWidget, TableColumn, TableView, TextInput,
    ToolBar, ToolButton,
};

#[cfg(test)]
mod tests {
    use super::*;
    use space_core::{Point, Rect};

    #[test]
    fn box_layout_distributes_fixed_and_expand() {
        let layout = BoxLayout::vbox(4, Insets::all(10));
        let rects = layout.compute(
            Rect::new(0, 0, 220, 200),
            &[SizePolicy::Fixed(30), SizePolicy::Expand, SizePolicy::Fixed(20)],
        );
        assert_eq!(rects.len(), 3);
        assert_eq!(rects[0], Rect::new(10, 10, 200, 30));
        assert_eq!(rects[1], Rect::new(10, 44, 200, 122));
        assert_eq!(rects[2], Rect::new(10, 170, 200, 20));
    }

    #[test]
    fn table_view_gates_keys_on_focus() {
        let mut tv = TableView::new(
            Rect::new(0, 0, 200, 150),
            vec![TableColumn::new("Name", 100)],
        );
        tv.set_rows(vec![vec!["a".into()], vec!["b".into()]]);
        // Unfocused: Enter must not activate
        assert!(!tv.on_event(UiEvent::Key { code: 28, pressed: true, ch: None, mods: 0 }));
        assert_eq!(tv.take_activated(), None);
        // Click row 0 focuses table
        assert!(tv.on_event(UiEvent::MouseDown { pos: Point::new(10, 30), button: 1 }));
        assert!(tv.focused);
        assert_eq!(tv.selected, Some(0));
        // Down + Enter while focused activates row 1
        assert!(tv.on_event(UiEvent::Key { code: 108, pressed: true, ch: None, mods: 0 }));
        assert!(tv.on_event(UiEvent::Key { code: 28, pressed: true, ch: None, mods: 0 }));
        assert_eq!(tv.take_activated(), Some(1));
        // Focus(false) clears focus
        assert!(tv.on_event(UiEvent::Focus(false)));
        assert!(!tv.focused);
    }

    #[test]
    fn text_input_clears_focus_on_focus_loss() {
        let mut ti = TextInput::new(Rect::new(0, 0, 150, 28), "Search");
        assert!(ti.on_event(UiEvent::MouseDown { pos: Point::new(10, 10), button: 1 }));
        assert!(ti.focused);
        assert!(ti.on_event(UiEvent::Focus(false)));
        assert!(!ti.focused);
    }
}
