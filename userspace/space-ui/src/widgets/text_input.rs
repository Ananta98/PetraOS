//! Single-line text input widget with sunken 3D frame and focus ring.

use crate::painter::StylePainter;
use crate::theme::Theme;
use crate::traits::{EventListener, Layoutable, Paintable, UiEvent};
use space_core::font;
use space_core::{Canvas, Rect};

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
        self.paint_with(canvas, &Theme::PETRA_SLATE);
    }

    fn paint_with(&self, canvas: &mut dyn Canvas, t: &Theme) {
        StylePainter::new(t).paint_sunken_field(canvas, self.rect, self.focused);

        let scale = 2;
        let clip = Rect::new(
            self.rect.x + 6,
            self.rect.y + 2,
            (self.rect.w - 12).max(0),
            (self.rect.h - 4).max(0),
        );
        let ty = self.rect.y + (self.rect.h - font::line_height(scale)) / 2;
        if self.text.is_empty() {
            font::draw_text_clipped(canvas, clip, self.rect.x + 8, ty, &self.placeholder, t.text_secondary, scale);
        } else {
            let tw = font::measure_text_width(&self.text, scale);
            let overflow = (tw - (clip.w - 6)).max(0);
            let tx = self.rect.x + 8 - overflow;
            font::draw_text_clipped(canvas, clip, tx, ty, &self.text, t.text_primary, scale);
            if self.focused {
                let cx = (tx + tw).clamp(clip.x, clip.right() - 2);
                canvas.fill_rect(Rect::new(cx, ty, 2, font::line_height(scale)), t.text_primary);
            }
        }
    }
}

impl EventListener for TextInput {
    fn on_event(&mut self, event: UiEvent) -> bool {
        match event {
            UiEvent::Focus(focused) => {
                if self.focused != focused {
                    self.focused = focused;
                    return true;
                }
            }
            UiEvent::MouseDown { pos, button: 1 } => {
                let was = self.focused;
                self.focused = self.rect.contains(pos);
                if was != self.focused {
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
