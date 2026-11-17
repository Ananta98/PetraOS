//! ToolBar and ToolButton widgets (SerenityOS `LibGUI::Toolbar` pattern).

use crate::painter::StylePainter;
use crate::theme::Theme;
use crate::traits::{EventListener, Layoutable, Paintable, UiEvent, WidgetState};
use space_core::font;
use space_core::icons::{self, IconId};
use space_core::{Canvas, Rect};

#[derive(Debug, Clone)]
pub struct ToolButton {
    pub id: &'static str,
    pub label: String,
    pub icon: Option<IconId>,
    pub show_label: bool,
    pub active: bool,
    pub enabled: bool,
}

impl ToolButton {
    pub fn new(id: &'static str, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            icon: None,
            show_label: true,
            active: false,
            enabled: true,
        }
    }

    pub fn with_icon(mut self, icon: impl Into<IconId>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn icon_only(mut self) -> Self {
        self.show_label = false;
        self
    }

    pub fn with_active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
}

#[derive(Debug, Clone)]
pub struct ToolBar {
    pub rect: Rect,
    pub buttons: Vec<ToolButton>,
    hovered: Option<usize>,
    pressed: Option<usize>,
    triggered: Option<&'static str>,
}

impl ToolBar {
    pub fn new(rect: Rect, buttons: Vec<ToolButton>) -> Self {
        Self {
            rect,
            buttons,
            hovered: None,
            pressed: None,
            triggered: None,
        }
    }

    pub fn take_action(&mut self) -> Option<&'static str> {
        self.triggered.take()
    }

    fn button_rect(&self, idx: usize) -> Rect {
        let mut x = self.rect.x + 6;
        let bh = (self.rect.h - 6).max(20);
        let icon_sz = (bh - 8).clamp(14, 18);
        for (i, b) in self.buttons.iter().enumerate() {
            let w = match (b.icon.is_some(), b.show_label && !b.label.is_empty()) {
                (true, false) => bh.max(26),
                (true, true) => (6 + icon_sz + 5 + font::measure_text_width(&b.label, 2) + 7).max(48),
                (false, _) => (font::measure_text_width(&b.label, 2) + 18).max(46),
            };
            if i == idx {
                return Rect::new(x, self.rect.y + 3, w, bh);
            }
            x += w + 3;
        }
        Rect::new(0, 0, 0, 0)
    }
}

impl Layoutable for ToolBar {
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
}

impl Paintable for ToolBar {
    fn paint(&self, canvas: &mut dyn Canvas) {
        self.paint_with(canvas, &Theme::PETRA_SLATE);
    }

    fn paint_with(&self, canvas: &mut dyn Canvas, t: &Theme) {
        let painter = StylePainter::new(t);
        painter.paint_raised_bar(canvas, self.rect);

        for (i, btn) in self.buttons.iter().enumerate() {
            let r = self.button_rect(i);
            if r.right() > self.rect.right() {
                break;
            }
            let state = if !btn.enabled {
                WidgetState::Disabled
            } else if self.pressed == Some(i) || btn.active {
                WidgetState::Pressed
            } else if self.hovered == Some(i) {
                WidgetState::Hovered
            } else {
                WidgetState::Normal
            };
            if state != WidgetState::Normal {
                painter.paint_button_frame(canvas, r, state, false);
            }
            let push = i32::from(self.pressed == Some(i) || btn.active);
            let fg = if btn.enabled { t.text_primary } else { t.text_secondary };
            let icon_sz = (r.h - 8).clamp(14, 18);
            let iy = r.y + (r.h - icon_sz) / 2 + push;
            let ty = r.y + (r.h - font::GLYPH_H * 2) / 2 + push;

            match (&btn.icon, btn.show_label && !btn.label.is_empty()) {
                (Some(icon), false) => {
                    let ix = r.x + (r.w - icon_sz) / 2;
                    icons::draw_icon(canvas, ix, iy, icon_sz, icon);
                }
                (Some(icon), true) => {
                    let tw = font::measure_text_width(&btn.label, 2);
                    let content_w = icon_sz + 5 + tw;
                    let sx = (r.x + (r.w - content_w) / 2).max(r.x + 5);
                    icons::draw_icon(canvas, sx, iy, icon_sz, icon);
                    font::draw_text_clipped(canvas, r, sx + icon_sz + 5, ty, &btn.label, fg, 2);
                }
                (None, _) => {
                    let tw = font::measure_text_width(&btn.label, 2);
                    let tx = r.x + (r.w - tw) / 2;
                    font::draw_text(canvas, tx, ty, &btn.label, fg, 2);
                }
            }
        }
    }
}

impl EventListener for ToolBar {
    fn on_event(&mut self, event: UiEvent) -> bool {
        match event {
            UiEvent::MouseMove { pos, .. } => {
                let next = (0..self.buttons.len())
                    .find(|&i| self.buttons[i].enabled && self.button_rect(i).contains(pos));
                if next != self.hovered {
                    self.hovered = next;
                    return true;
                }
            }
            UiEvent::MouseDown { pos, button: 1 } => {
                if let Some(i) = (0..self.buttons.len())
                    .find(|&i| self.buttons[i].enabled && self.button_rect(i).contains(pos))
                {
                    self.pressed = Some(i);
                    return true;
                }
            }
            UiEvent::MouseUp { pos, button: 1 } => {
                if let Some(i) = self.pressed.take() {
                    if self.buttons[i].enabled && self.button_rect(i).contains(pos) {
                        self.triggered = Some(self.buttons[i].id);
                    }
                    return true;
                }
            }
            _ => {}
        }
        false
    }
}
