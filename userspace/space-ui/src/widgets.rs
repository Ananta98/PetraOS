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

/// Draws a classic 3D raised or sunken panel/bevel using the given theme.
pub fn draw_panel_3d(canvas: &mut dyn Canvas, rect: Rect, sunken: bool, t: &Theme) {
    if rect.w < 4 || rect.h < 4 {
        return;
    }
    let (tl_outer, br_outer, tl_inner, br_inner) = if sunken {
        (t.bevel_dark, t.bevel_light, t.bevel_shadow, t.bg_window)
    } else {
        (t.bevel_light, t.bevel_dark, t.bg_window, t.bevel_shadow)
    };
    // Outer 1px bevel
    canvas.fill_rect(Rect::new(rect.x, rect.y, rect.w - 1, 1), tl_outer);
    canvas.fill_rect(Rect::new(rect.x, rect.y, 1, rect.h - 1), tl_outer);
    canvas.fill_rect(Rect::new(rect.x, rect.bottom() - 1, rect.w, 1), br_outer);
    canvas.fill_rect(Rect::new(rect.right() - 1, rect.y, 1, rect.h), br_outer);
    // Inner 1px bevel
    canvas.fill_rect(Rect::new(rect.x + 1, rect.y + 1, rect.w - 3, 1), tl_inner);
    canvas.fill_rect(Rect::new(rect.x + 1, rect.y + 1, 1, rect.h - 3), tl_inner);
    canvas.fill_rect(Rect::new(rect.x + 1, rect.bottom() - 2, rect.w - 2, 1), br_inner);
    canvas.fill_rect(Rect::new(rect.right() - 2, rect.y + 1, 1, rect.h - 2), br_inner);
}

impl Paintable for Button {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::XP_LUNA;
        let sunken = self.pressed || self.active;
        let inner = Rect::new(self.rect.x + 1, self.rect.y + 1, (self.rect.w - 2).max(1), (self.rect.h - 2).max(1));

        if sunken {
            let top = if self.active && !self.pressed {
                Color::rgb(0xD8, 0xE4, 0xF8)
            } else {
                Color::rgb(0xD6, 0xD2, 0xC2)
            };
            let bot = Color::rgb(0xEC, 0xE9, 0xD8);
            canvas.fill_gradient_v(inner, top, bot);
            canvas.draw_rect_outline(self.rect, 1, Color::rgb(0x00, 0x3C, 0x74));
            draw_panel_3d(canvas, inner, true, &t);
        } else {
            let (top, bot) = if self.accent {
                (Color::rgb(0xFF, 0xFF, 0xFF), Color::rgb(0xD6, 0xE4, 0xFA))
            } else if self.hovered {
                (Color::rgb(0xFF, 0xFA, 0xE6), Color::rgb(0xEE, 0xE6, 0xCC))
            } else {
                (Color::rgb(0xFF, 0xFF, 0xFF), Color::rgb(0xDC, 0xD8, 0xC8))
            };
            canvas.fill_gradient_v(inner, top, bot);
            let outer_col = if self.hovered || self.accent {
                Color::rgb(0x00, 0x3C, 0x74)
            } else {
                Color::rgb(0x7F, 0x9D, 0xB9)
            };
            canvas.draw_rect_outline(self.rect, 1, outer_col);
            draw_panel_3d(canvas, inner, false, &t);
            if self.hovered {
                canvas.draw_rect_outline(
                    Rect::new(self.rect.x + 1, self.rect.y + 1, self.rect.w - 2, self.rect.h - 2),
                    1,
                    t.warning,
                );
            }
        }

        let scale = 2;
        let tw = font::measure_text_width(&self.label, scale);
        let th = font::GLYPH_H * scale;
        let off = if sunken { 1 } else { 0 };
        let tx = self.rect.x + (self.rect.w - tw) / 2 + off;
        let ty = self.rect.y + (self.rect.h - th) / 2 + off;
        font::draw_text(canvas, tx.max(self.rect.x + 4), ty, &self.label, t.text_primary, scale);
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
        let t = Theme::XP_LUNA;
        canvas.fill_rect(self.rect, t.bg_input);
        draw_panel_3d(canvas, self.rect, true, &t);
        if self.focused {
            canvas.draw_rect_outline(self.rect, 1, t.accent);
        } else {
            canvas.draw_rect_outline(self.rect, 1, t.border);
        }

        let scale = 2;
        let ty = self.rect.y + (self.rect.h - font::GLYPH_H * scale) / 2;
        if self.text.is_empty() {
            font::draw_text(canvas, self.rect.x + 8, ty, &self.placeholder, t.text_secondary, scale);
        } else {
            font::draw_text(canvas, self.rect.x + 8, ty, &self.text, t.text_primary, scale);
            if self.focused {
                let cx = self.rect.x + 8 + font::measure_text_width(&self.text, scale);
                canvas.fill_rect(Rect::new(cx, ty - 1, 2, font::GLYPH_H * scale + 2), t.text_primary);
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
    let t = Theme::XP_LUNA;
    canvas.fill_rect(rect, t.bg_input);
    draw_panel_3d(canvas, rect, true, &t);
    canvas.draw_rect_outline(rect, 1, t.border);
    let clamped = ratio.clamp(0.0, 1.0);
    let fill_w = ((rect.w - 6) as f32 * clamped) as i32;
    if fill_w > 0 {
        let fill_r = Rect::new(rect.x + 3, rect.y + 3, fill_w, rect.h - 6);
        let top = Color::lerp(fill_color, Color::WHITE, 1, 3);
        canvas.fill_gradient_v(fill_r, top, fill_color);
    }
    if !label.is_empty() {
        let ty = rect.y + (rect.h - font::GLYPH_H * 2) / 2;
        font::draw_text(canvas, rect.x + 8, ty + 1, label, Color::argb(90, 255, 255, 255), 2);
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
