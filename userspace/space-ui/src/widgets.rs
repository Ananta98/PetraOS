//! Reusable polymorphic widgets adhering to LSP and ISP.

use crate::theme::Theme;
use crate::traits::{EventListener, Layoutable, Paintable, UiEvent};
use space_core::font;
use space_core::{Canvas, Color, Point, Rect};

/// Clickable button widget.
#[derive(Debug, Clone)]
pub struct Button {
    pub rect: Rect,
    pub label: String,
    pub hovered: bool,
    pub pressed: bool,
    pub active: bool,
    pub clicked: bool,
    pub accent: bool,
}

impl Button {
    pub fn new(rect: Rect, label: impl Into<String>) -> Self {
        Self {
            rect,
            label: label.into(),
            hovered: false,
            pressed: false,
            active: false,
            clicked: false,
            accent: false,
        }
    }

    pub fn take_click(&mut self) -> bool {
        let c = self.clicked;
        self.clicked = false;
        c
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
        let t = Theme::BREEZE_DARK;
        let bg = if self.pressed || self.active || self.accent {
            t.accent
        } else if self.hovered {
            t.bg_hover
        } else {
            t.bg_surface_alt
        };
        let fg = if self.pressed || self.active || self.accent {
            t.text_on_accent
        } else {
            t.text_primary
        };
        canvas.fill_rounded_rect(self.rect, t.radius, bg);
        canvas.draw_rect_outline(self.rect, 1, t.border);

        let scale = 2;
        let tw = font::measure_text_width(&self.label, scale);
        let th = font::GLYPH_H * scale;
        let tx = self.rect.x + (self.rect.w - tw) / 2;
        let ty = self.rect.y + (self.rect.h - th) / 2;
        font::draw_text(canvas, tx.max(self.rect.x + 4), ty, &self.label, fg, scale);
    }
}

impl EventListener for Button {
    fn on_event(&mut self, event: UiEvent) -> bool {
        match event {
            UiEvent::MouseMove { pos, .. } => {
                let inside = self.rect.contains(pos);
                if inside != self.hovered {
                    self.hovered = inside;
                    return true;
                }
            }
            UiEvent::MouseDown { pos, button } if button == 1 => {
                if self.rect.contains(pos) {
                    self.pressed = true;
                    return true;
                }
            }
            UiEvent::MouseUp { pos, button } if button == 1 => {
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

/// Single-line editable text input box.
#[derive(Debug, Clone)]
pub struct TextInput {
    pub rect: Rect,
    pub text: String,
    pub placeholder: String,
    pub focused: bool,
    pub submitted: bool,
}

impl TextInput {
    pub fn new(rect: Rect, placeholder: impl Into<String>) -> Self {
        Self {
            rect,
            text: String::new(),
            placeholder: placeholder.into(),
            focused: false,
            submitted: false,
        }
    }

    pub fn take_submit(&mut self) -> bool {
        let s = self.submitted;
        self.submitted = false;
        s
    }
}

impl Layoutable for TextInput {
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
}

impl Paintable for TextInput {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::BREEZE_DARK;
        canvas.fill_rounded_rect(self.rect, 4, Color::rgb(20, 23, 29));
        let border = if self.focused { t.accent } else { t.border };
        canvas.draw_rect_outline(self.rect, 1, border);

        let scale = 2;
        let ty = self.rect.y + (self.rect.h - font::GLYPH_H * scale) / 2;
        if self.text.is_empty() {
            font::draw_text(canvas, self.rect.x + 8, ty, &self.placeholder, t.text_secondary, scale);
        } else {
            font::draw_text(canvas, self.rect.x + 8, ty, &self.text, t.text_primary, scale);
            if self.focused {
                let cx = self.rect.x + 8 + font::measure_text_width(&self.text, scale);
                canvas.fill_rect(Rect::new(cx, ty - 1, 2, font::GLYPH_H * scale + 2), t.accent);
            }
        }
    }
}

impl EventListener for TextInput {
    fn on_event(&mut self, event: UiEvent) -> bool {
        match event {
            UiEvent::MouseDown { pos, .. } => {
                let inside = self.rect.contains(pos);
                if inside != self.focused {
                    self.focused = inside;
                    return true;
                }
            }
            UiEvent::Key { code, pressed: true, ch, .. } if self.focused => {
                if code == 14 {
                    self.text.pop();
                    return true;
                } else if code == 28 {
                    self.submitted = true;
                    return true;
                } else if let Some(c) = ch {
                    if !c.is_control() {
                        self.text.push(c);
                        return true;
                    }
                }
            }
            _ => {}
        }
        false
    }
}

/// Progress bar widget.
pub fn draw_progress_bar(canvas: &mut dyn Canvas, rect: Rect, ratio: f32, label: &str, fill_color: Color) {
    let t = Theme::BREEZE_DARK;
    canvas.fill_rounded_rect(rect, 4, Color::rgb(20, 23, 29));
    canvas.draw_rect_outline(rect, 1, t.border);
    let clamped = ratio.clamp(0.0, 1.0);
    let fill_w = ((rect.w - 4) as f32 * clamped) as i32;
    if fill_w > 0 {
        canvas.fill_rounded_rect(Rect::new(rect.x + 2, rect.y + 2, fill_w, rect.h - 4), 3, fill_color);
    }
    if !label.is_empty() {
        let ty = rect.y + (rect.h - font::GLYPH_H * 2) / 2;
        font::draw_text(canvas, rect.x + 8, ty, label, t.text_primary, 2);
    }
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
