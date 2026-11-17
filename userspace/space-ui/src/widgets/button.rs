//! Clickable button widget and legacy helper wrappers.

use crate::painter::StylePainter;
use crate::theme::Theme;
use crate::traits::{EventListener, Layoutable, Paintable, UiEvent, WidgetState};
use space_core::font;
use space_core::icons::{self, IconId};
use space_core::{Canvas, Color, Point, Rect};

/// Clickable button widget with hover, pressed, and accent states.
#[derive(Debug, Clone)]
pub struct Button {
    pub rect: Rect,
    pub label: String,
    pub icon: Option<IconId>,
    pub hovered: bool,
    pub pressed: bool,
    pub active: bool,
    pub clicked: bool,
    pub accent: bool,
    pub disabled: bool,
}

impl Button {
    pub fn new(rect: Rect, label: impl Into<String>) -> Self {
        Self {
            rect,
            label: label.into(),
            icon: None,
            hovered: false,
            pressed: false,
            active: false,
            clicked: false,
            accent: false,
            disabled: false,
        }
    }

    pub fn with_icon(mut self, icon: impl Into<IconId>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn take_click(&mut self) -> bool {
        let c = self.clicked;
        self.clicked = false;
        c
    }

    fn state(&self) -> WidgetState {
        if self.disabled {
            WidgetState::Disabled
        } else if self.pressed || self.active {
            WidgetState::Pressed
        } else if self.hovered {
            WidgetState::Hovered
        } else {
            WidgetState::Normal
        }
    }
}

impl Layoutable for Button {
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
}

impl Paintable for Button {
    fn paint(&self, canvas: &mut dyn Canvas) {
        self.paint_with(canvas, &Theme::PETRA_SLATE);
    }

    fn paint_with(&self, canvas: &mut dyn Canvas, t: &Theme) {
        let painter = StylePainter::new(t);
        painter.paint_button_frame(canvas, self.rect, self.state(), self.accent);

        let scale = if self.rect.h >= 34 { 2 } else { 1 };
        let th = font::GLYPH_H * scale;
        let push = i32::from(self.pressed || self.active);
        let fg = if self.disabled {
            t.text_secondary
        } else if self.accent {
            t.text_on_accent
        } else {
            t.text_primary
        };

        if let Some(ref icon) = self.icon {
            let icon_sz = (self.rect.h - 10).clamp(12, 18);
            let iy = self.rect.y + (self.rect.h - icon_sz) / 2 + push;
            if self.label.is_empty() {
                let ix = self.rect.x + (self.rect.w - icon_sz) / 2;
                icons::draw_icon(canvas, ix, iy, icon_sz, icon);
            } else {
                let gap = 5;
                let tw = font::measure_text_width(&self.label, scale);
                let content_w = icon_sz + gap + tw;
                let sx = (self.rect.x + (self.rect.w - content_w) / 2).max(self.rect.x + 5);
                let ty = self.rect.y + (self.rect.h - th) / 2 + push;
                icons::draw_icon(canvas, sx, iy, icon_sz, icon);
                font::draw_text_clipped(canvas, self.rect, sx + icon_sz + gap, ty, &self.label, fg, scale);
            }
        } else {
            let tw = font::measure_text_width(&self.label, scale);
            let tx = self.rect.x + (self.rect.w - tw) / 2;
            let ty = self.rect.y + (self.rect.h - th) / 2 + push;
            font::draw_text_clipped(canvas, self.rect, tx.max(self.rect.x + 4), ty, &self.label, fg, scale);
        }
    }
}

impl EventListener for Button {
    fn on_event(&mut self, event: UiEvent) -> bool {
        if self.disabled {
            return false;
        }
        match event {
            UiEvent::MouseMove { pos, .. } => {
                let inside = self.rect.contains(pos);
                if inside != self.hovered {
                    self.hovered = inside;
                    return true;
                }
            }
            UiEvent::MouseDown { pos, button: 1 } => {
                if self.rect.contains(pos) {
                    self.pressed = true;
                    return true;
                }
            }
            UiEvent::MouseUp { pos, button: 1 } => {
                if self.pressed {
                    self.pressed = false;
                    if self.rect.contains(pos) {
                        self.clicked = true;
                    }
                    return true;
                }
            }
            _ => {}
        }
        false
    }
}

/// Draws a classic 3D raised or sunken panel/bevel using `StylePainter`.
pub fn draw_panel_3d(canvas: &mut dyn Canvas, rect: Rect, sunken: bool, t: &Theme) {
    StylePainter::new(t).paint_panel_3d(canvas, rect, sunken);
}

/// Progress bar widget.
pub fn draw_progress_bar(canvas: &mut dyn Canvas, rect: Rect, ratio: f32, label: &str, fill_color: Color) {
    let t = Theme::PETRA_SLATE;
    StylePainter::new(&t).paint_progress_bar(canvas, rect, ratio, label, fill_color);
}

/// Scrollable list hit-test helper.
pub fn list_item_at(rect: Rect, row_h: i32, scroll: usize, total: usize, pos: Point) -> Option<usize> {
    if !rect.contains(pos) || row_h <= 0 {
        return None;
    }
    let idx = scroll + ((pos.y - rect.y) / row_h) as usize;
    if idx < total {
        Some(idx)
    } else {
        None
    }
}
