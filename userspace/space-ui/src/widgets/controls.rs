//! CheckBox, TabWidget, and Splitter controls (SerenityOS `LibGUI` patterns).

use crate::painter::StylePainter;
use crate::theme::Theme;
use crate::traits::{EventListener, Layoutable, Paintable, UiEvent};
use space_core::font;
use space_core::{Canvas, Color, Rect};

#[derive(Debug, Clone)]
pub struct CheckBox {
    pub rect: Rect,
    pub label: String,
    pub checked: bool,
    pub changed: bool,
}

impl CheckBox {
    pub fn new(rect: Rect, label: impl Into<String>, checked: bool) -> Self {
        Self {
            rect,
            label: label.into(),
            checked,
            changed: false,
        }
    }

    pub fn take_changed(&mut self) -> bool {
        let c = self.changed;
        self.changed = false;
        c
    }
}

impl Layoutable for CheckBox {
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
}

impl Paintable for CheckBox {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::PETRA_SLATE;
        let box_r = Rect::new(self.rect.x, self.rect.y + (self.rect.h - 16) / 2, 16, 16);
        StylePainter::new(&t).paint_sunken_field(canvas, box_r, false);
        if self.checked {
            let check_col = t.accent;
            canvas.fill_rect(Rect::new(box_r.x + 4, box_r.y + 8, 3, 4), check_col);
            canvas.fill_rect(Rect::new(box_r.x + 7, box_r.y + 6, 3, 6), check_col);
            canvas.fill_rect(Rect::new(box_r.x + 10, box_r.y + 4, 3, 5), check_col);
        }
        let ty = self.rect.y + (self.rect.h - font::GLYPH_H * 2) / 2;
        font::draw_text(canvas, box_r.x + 24, ty, &self.label, t.text_primary, 2);
    }
}

impl EventListener for CheckBox {
    fn on_event(&mut self, event: UiEvent) -> bool {
        if let UiEvent::MouseDown { pos, button: 1 } = event {
            if self.rect.contains(pos) {
                self.checked = !self.checked;
                self.changed = true;
                return true;
            }
        }
        false
    }
}

/// Tab strip widget for multi-page views (e.g. Control Panel).
#[derive(Debug, Clone)]
pub struct TabWidget {
    pub rect: Rect,
    pub tabs: Vec<String>,
    pub active: usize,
    pub changed: bool,
}

impl TabWidget {
    pub fn new(rect: Rect, tabs: Vec<String>) -> Self {
        Self {
            rect,
            tabs,
            active: 0,
            changed: false,
        }
    }

    pub fn take_changed(&mut self) -> bool {
        let c = self.changed;
        self.changed = false;
        c
    }

    fn tab_rect(&self, idx: usize) -> Rect {
        let mut x = self.rect.x + 6;
        for (i, title) in self.tabs.iter().enumerate() {
            let w = (font::measure_text_width(title, 2) + 22).max(70);
            if i == idx {
                let top_off = if self.active == i { 0 } else { 3 };
                return Rect::new(x, self.rect.y + top_off, w, (self.rect.h - top_off).max(20));
            }
            x += w + 3;
        }
        Rect::new(0, 0, 0, 0)
    }
}

impl Layoutable for TabWidget {
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
}

impl Paintable for TabWidget {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::PETRA_SLATE;
        let painter = StylePainter::new(&t);
        canvas.fill_rect(
            Rect::new(self.rect.x, self.rect.y + self.rect.h - 1, self.rect.w, 1),
            t.border,
        );
        for (i, title) in self.tabs.iter().enumerate() {
            let tr = self.tab_rect(i);
            if tr.right() > self.rect.right() {
                break;
            }
            let active = i == self.active;
            let bg = if active { t.bg_surface } else { t.bg_surface_alt };
            canvas.fill_rounded_rect(tr, 2, bg);
            canvas.draw_rect_outline(tr, 1, t.border);
            if active {
                canvas.fill_rect(Rect::new(tr.x + 1, tr.y, tr.w - 2, 2), t.accent);
                canvas.fill_rect(Rect::new(tr.x + 1, tr.y + tr.h - 1, tr.w - 2, 1), t.bg_surface);
            }
            let tw = font::measure_text_width(title, 2);
            let tx = tr.x + (tr.w - tw) / 2;
            let ty = tr.y + (tr.h - font::GLYPH_H * 2) / 2;
            if active {
                font::draw_text_bold(canvas, tx, ty, title, t.text_primary, 2);
            } else {
                font::draw_text(canvas, tx, ty, title, t.text_secondary, 2);
            }
        }
        let _ = painter;
    }
}

impl EventListener for TabWidget {
    fn on_event(&mut self, event: UiEvent) -> bool {
        if let UiEvent::MouseDown { pos, button: 1 } = event {
            if let Some(i) = (0..self.tabs.len()).find(|&i| self.tab_rect(i).contains(pos)) {
                if self.active != i {
                    self.active = i;
                    self.changed = true;
                    return true;
                }
            }
        }
        false
    }
}

/// Draggable vertical splitter dividing two horizontal panes.
#[derive(Debug, Clone)]
pub struct Splitter {
    pub rect: Rect,
    pub ratio: f32,
    dragging: bool,
}

impl Splitter {
    pub fn new(rect: Rect, ratio: f32) -> Self {
        Self {
            rect,
            ratio: ratio.clamp(0.15, 0.85),
            dragging: false,
        }
    }

    pub fn split_x(&self) -> i32 {
        self.rect.x + ((self.rect.w as f32) * self.ratio) as i32
    }

    pub fn left_pane(&self) -> Rect {
        let sx = self.split_x();
        Rect::new(self.rect.x, self.rect.y, (sx - self.rect.x - 2).max(0), self.rect.h)
    }

    pub fn right_pane(&self) -> Rect {
        let sx = self.split_x() + 3;
        Rect::new(sx, self.rect.y, (self.rect.x + self.rect.w - sx).max(0), self.rect.h)
    }
}

impl Layoutable for Splitter {
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
}

impl Paintable for Splitter {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::PETRA_SLATE;
        let sx = self.split_x() - 2;
        StylePainter::new(&t).paint_divider(canvas, Rect::new(sx, self.rect.y, 4, self.rect.h), true);
    }
}

impl EventListener for Splitter {
    fn on_event(&mut self, event: UiEvent) -> bool {
        let sx = self.split_x();
        let handle = Rect::new(sx - 4, self.rect.y, 8, self.rect.h);
        match event {
            UiEvent::MouseDown { pos, button: 1 } if handle.contains(pos) => {
                self.dragging = true;
                return true;
            }
            UiEvent::MouseMove { pos, .. } if self.dragging => {
                let next = ((pos.x - self.rect.x) as f32 / self.rect.w.max(1) as f32).clamp(0.15, 0.85);
                if (next - self.ratio).abs() > 0.005 {
                    self.ratio = next;
                    return true;
                }
            }
            UiEvent::MouseUp { button: 1, .. } if self.dragging => {
                self.dragging = false;
                return true;
            }
            _ => {}
        }
        false
    }
}


/// Theme-aware ProgressBar widget (`Layoutable + Paintable`).
#[derive(Debug, Clone)]
pub struct ProgressBar {
    pub rect: Rect,
    pub ratio: f32,
    pub label: String,
    pub fill: Option<Color>,
}

impl ProgressBar {
    pub fn new(rect: Rect, ratio: f32, label: impl Into<String>) -> Self {
        Self {
            rect,
            ratio,
            label: label.into(),
            fill: None,
        }
    }

    pub fn with_fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }
}

impl Layoutable for ProgressBar {
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
}

impl Paintable for ProgressBar {
    fn paint(&self, canvas: &mut dyn Canvas) {
        self.paint_with(canvas, &Theme::PETRA_SLATE);
    }

    fn paint_with(&self, canvas: &mut dyn Canvas, t: &Theme) {
        let fill = self.fill.unwrap_or(t.accent);
        StylePainter::new(t).paint_progress_bar(canvas, self.rect, self.ratio, &self.label, fill);
    }
}
