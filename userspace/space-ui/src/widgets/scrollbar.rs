//! ScrollBar widget (SerenityOS `LibGUI::Scrollbar` pattern) with thumb dragging & wheel.

use crate::painter::StylePainter;
use crate::theme::Theme;
use crate::traits::{EventListener, Layoutable, Paintable, UiEvent};
use space_core::{Canvas, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone)]
pub struct ScrollBar {
    pub rect: Rect,
    pub orientation: Orientation,
    pub value: usize,
    pub max: usize,
    pub page: usize,
    dragging: bool,
    drag_anchor: i32,
    drag_start_val: usize,
}

impl ScrollBar {
    pub fn new(rect: Rect, orientation: Orientation) -> Self {
        Self {
            rect,
            orientation,
            value: 0,
            max: 0,
            page: 1,
            dragging: false,
            drag_anchor: 0,
            drag_start_val: 0,
        }
    }

    pub fn set_range(&mut self, total: usize, page: usize) {
        self.page = page.max(1);
        self.max = total.saturating_sub(self.page);
        if self.value > self.max {
            self.value = self.max;
        }
    }

    pub fn scroll_by(&mut self, delta: i32) -> bool {
        let next = if delta < 0 {
            self.value.saturating_sub((-delta) as usize)
        } else {
            (self.value + delta as usize).min(self.max)
        };
        if next != self.value {
            self.value = next;
            true
        } else {
            false
        }
    }

    fn thumb_rect(&self) -> Rect {
        let track_len = match self.orientation {
            Orientation::Vertical => self.rect.h,
            Orientation::Horizontal => self.rect.w,
        };
        let total = (self.max + self.page).max(1);
        let thumb_len = ((track_len as usize * self.page) / total)
            .max(20)
            .min(track_len.max(1) as usize) as i32;
        let travel = (track_len - thumb_len).max(0);
        let offset = if self.max > 0 {
            ((travel as usize * self.value) / self.max) as i32
        } else {
            0
        };

        match self.orientation {
            Orientation::Vertical => Rect::new(self.rect.x + 1, self.rect.y + offset, self.rect.w - 2, thumb_len),
            Orientation::Horizontal => Rect::new(self.rect.x + offset, self.rect.y + 1, thumb_len, self.rect.h - 2),
        }
    }
}

impl Layoutable for ScrollBar {
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
}

impl Paintable for ScrollBar {
    fn paint(&self, canvas: &mut dyn Canvas) {
        self.paint_with(canvas, &Theme::PETRA_SLATE);
    }

    fn paint_with(&self, canvas: &mut dyn Canvas, t: &Theme) {
        canvas.fill_rect(self.rect, t.bg_surface_alt);
        canvas.draw_rect_outline(self.rect, 1, t.border);

        let thumb = self.thumb_rect();
        canvas.fill_gradient_v(thumb, t.bg_surface, t.bevel_shadow);
        StylePainter::new(t).paint_panel_3d(canvas, thumb, false);
    }
}

impl EventListener for ScrollBar {
    fn on_event(&mut self, event: UiEvent) -> bool {
        match event {
            UiEvent::MouseDown { pos, button: 1 } => {
                if !self.rect.contains(pos) {
                    return false;
                }
                let thumb = self.thumb_rect();
                if thumb.contains(pos) {
                    self.dragging = true;
                    self.drag_anchor = match self.orientation {
                        Orientation::Vertical => pos.y,
                        Orientation::Horizontal => pos.x,
                    };
                    self.drag_start_val = self.value;
                    return true;
                }
                // Click on track: jump one page toward click
                let before = match self.orientation {
                    Orientation::Vertical => pos.y < thumb.y,
                    Orientation::Horizontal => pos.x < thumb.x,
                };
                let step = self.page.max(1) as i32;
                return self.scroll_by(if before { -step } else { step });
            }
            UiEvent::MouseMove { pos, .. } if self.dragging => {
                let thumb = self.thumb_rect();
                let (track_len, thumb_len, cur) = match self.orientation {
                    Orientation::Vertical => (self.rect.h, thumb.h, pos.y),
                    Orientation::Horizontal => (self.rect.w, thumb.w, pos.x),
                };
                let travel = (track_len - thumb_len).max(1);
                let delta_px = cur - self.drag_anchor;
                let delta_val = (delta_px as isize * self.max as isize) / travel as isize;
                let next = (self.drag_start_val as isize + delta_val)
                    .clamp(0, self.max as isize) as usize;
                if next != self.value {
                    self.value = next;
                    return true;
                }
            }
            UiEvent::MouseUp { button: 1, .. } if self.dragging => {
                self.dragging = false;
                return true;
            }
            UiEvent::Wheel { pos, delta } if self.rect.contains(pos) => {
                return self.scroll_by(-delta * 3);
            }
            _ => {}
        }
        false
    }
}
