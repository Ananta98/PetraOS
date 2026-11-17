//! StatusBar widget (SerenityOS `LibGUI::Statusbar` pattern) with sunken segments.

use crate::painter::StylePainter;
use crate::theme::Theme;
use crate::traits::{Layoutable, Paintable};
use space_core::font;
use space_core::{Canvas, Rect};

#[derive(Debug, Clone)]
pub struct StatusBar {
    pub rect: Rect,
    pub segments: Vec<String>,
}

impl StatusBar {
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            segments: vec![String::new()],
        }
    }

    pub fn set_text(&mut self, idx: usize, text: impl Into<String>) {
        if idx >= self.segments.len() {
            self.segments.resize(idx + 1, String::new());
        }
        self.segments[idx] = text.into();
    }
}

impl Layoutable for StatusBar {
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
}

impl Paintable for StatusBar {
    fn paint(&self, canvas: &mut dyn Canvas) {
        self.paint_with(canvas, &Theme::XP_LUNA);
    }

    fn paint_with(&self, canvas: &mut dyn Canvas, t: &Theme) {
        if self.rect.w < 16 || self.rect.h < 10 {
            return;
        }
        let painter = StylePainter::new(t);
        painter.paint_raised_bar(canvas, self.rect);

        let count = self.segments.len().max(1) as i32;
        let seg_w = (self.rect.w - 8) / count;
        let ty = self.rect.y + (self.rect.h - font::GLYPH_H * 2) / 2;

        for (i, seg) in self.segments.iter().enumerate() {
            let sx = self.rect.x + 4 + (i as i32) * seg_w;
            let sr = Rect::new(sx, self.rect.y + 3, (seg_w - 4).max(8), self.rect.h - 6);
            painter.paint_panel_3d(canvas, sr, true);
            font::draw_text_clipped(canvas, sr, sr.x + 6, ty, seg, t.text_primary, 2);
        }
    }
}
