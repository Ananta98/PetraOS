//! TableView widget (SerenityOS `LibGUI::TableView` pattern) with column headers,
//! alternating rows, keyboard Up/Down/Enter navigation, and integrated ScrollBar.

use super::scrollbar::{Orientation, ScrollBar};
use crate::painter::StylePainter;
use crate::theme::Theme;
use crate::traits::{EventListener, Layoutable, Paintable, UiEvent};
use space_core::font;
use space_core::{Canvas, Rect};

const HEADER_H: i32 = 24;
const SCROLLBAR_W: i32 = 16;

#[derive(Debug, Clone)]
pub struct TableColumn {
    pub title: String,
    pub width: i32,
}

impl TableColumn {
    pub fn new(title: impl Into<String>, width: i32) -> Self {
        Self {
            title: title.into(),
            width,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TableView {
    pub rect: Rect,
    pub columns: Vec<TableColumn>,
    pub rows: Vec<Vec<String>>,
    pub selected: Option<usize>,
    pub activated: Option<usize>,
    pub focused: bool,
    pub row_height: i32,
    pub scrollbar: ScrollBar,
}

impl TableView {
    pub fn new(rect: Rect, columns: Vec<TableColumn>) -> Self {
        let mut tv = Self {
            rect,
            columns,
            rows: Vec::new(),
            selected: None,
            activated: None,
            focused: false,
            row_height: 24,
            scrollbar: ScrollBar::new(Rect::new(0, 0, 0, 0), Orientation::Vertical),
        };
        tv.sync_scrollbar();
        tv
    }

    pub fn set_rows(&mut self, rows: Vec<Vec<String>>) {
        self.rows = rows;
        if let Some(sel) = self.selected {
            if sel >= self.rows.len() {
                self.selected = self.rows.len().checked_sub(1);
            }
        }
        self.sync_scrollbar();
    }

    pub fn take_activated(&mut self) -> Option<usize> {
        self.activated.take()
    }

    fn body_rect(&self) -> Rect {
        Rect::new(
            self.rect.x + 2,
            self.rect.y + HEADER_H,
            (self.rect.w - 4 - SCROLLBAR_W).max(0),
            (self.rect.h - HEADER_H - 2).max(0),
        )
    }

    fn visible_rows(&self) -> usize {
        (self.body_rect().h / self.row_height.max(1)).max(1) as usize
    }

    fn sync_scrollbar(&mut self) {
        let sb_rect = Rect::new(
            self.rect.x + self.rect.w - SCROLLBAR_W - 2,
            self.rect.y + HEADER_H,
            SCROLLBAR_W,
            (self.rect.h - HEADER_H - 2).max(0),
        );
        self.scrollbar.set_bounds(sb_rect);
        self.scrollbar.set_range(self.rows.len(), self.visible_rows());
    }

    fn ensure_visible(&mut self, idx: usize) {
        let vis = self.visible_rows();
        if idx < self.scrollbar.value {
            self.scrollbar.value = idx;
        } else if idx >= self.scrollbar.value + vis {
            self.scrollbar.value = (idx + 1).saturating_sub(vis);
        }
    }
}

impl Layoutable for TableView {
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
        self.sync_scrollbar();
    }
}

impl Paintable for TableView {
    fn paint(&self, canvas: &mut dyn Canvas) {
        self.paint_with(canvas, &Theme::XP_LUNA);
    }

    fn paint_with(&self, canvas: &mut dyn Canvas, t: &Theme) {
        let painter = StylePainter::new(t);
        painter.paint_sunken_field(canvas, self.rect, self.focused);

        let hdr = Rect::new(self.rect.x + 2, self.rect.y + 2, (self.rect.w - 4).max(0), HEADER_H - 2);
        painter.paint_raised_bar(canvas, hdr);
        let mut cx = hdr.x + 6;
        for (i, col) in self.columns.iter().enumerate() {
            let avail_w = (hdr.right() - cx).min(col.width - 8);
            if avail_w > 0 {
                let clip = Rect::new(cx, hdr.y, avail_w, hdr.h);
                let ty = hdr.y + (hdr.h - font::line_height(2)) / 2;
                font::draw_text_bold_clipped(canvas, clip, cx, ty, &col.title, t.text_primary, 2);
            }
            cx += col.width;
            if i + 1 < self.columns.len() && cx - 4 < hdr.right() {
                painter.paint_divider(canvas, Rect::new(cx - 4, hdr.y + 2, 2, hdr.h - 4), true);
            }
        }

        let body = self.body_rect();
        let vis = self.visible_rows();
        let start = self.scrollbar.value;
        for slot in 0..vis {
            let row_idx = start + slot;
            if row_idx >= self.rows.len() {
                break;
            }
            let ry = body.y + (slot as i32) * self.row_height;
            if ry + self.row_height > body.y + body.h {
                break;
            }
            let rr = Rect::new(body.x, ry, body.w, self.row_height);
            let sel = self.selected == Some(row_idx);
            painter.paint_row_bg(canvas, rr, row_idx, sel);
            let fg = if sel { t.text_on_accent } else { t.text_primary };

            let mut x = body.x + 6;
            for (ci, col) in self.columns.iter().enumerate() {
                let avail_w = (body.right() - x).min(col.width - 8);
                if avail_w > 0 {
                    if let Some(cell) = self.rows[row_idx].get(ci) {
                        let clip = Rect::new(x, ry, avail_w, self.row_height);
                        let ty = ry + (self.row_height - font::line_height(2)) / 2;
                        font::draw_text_clipped(canvas, clip, x, ty, cell, fg, 2);
                    }
                }
                x += col.width;
            }
        }

        self.scrollbar.paint_with(canvas, t);
    }
}

impl EventListener for TableView {
    fn on_event(&mut self, event: UiEvent) -> bool {
        if self.scrollbar.on_event(event) {
            return true;
        }
        match event {
            UiEvent::Focus(focused) => {
                if !focused && self.focused {
                    self.focused = false;
                    return true;
                }
            }
            UiEvent::MouseDown { pos, button: 1 } => {
                let was_focused = self.focused;
                self.focused = self.rect.contains(pos);
                let body = self.body_rect();
                if body.contains(pos) {
                    let idx = self.scrollbar.value + ((pos.y - body.y) / self.row_height.max(1)) as usize;
                    if idx < self.rows.len() {
                        if self.selected == Some(idx) {
                            self.activated = Some(idx);
                        } else {
                            self.selected = Some(idx);
                        }
                        return true;
                    }
                }
                if was_focused != self.focused {
                    return true;
                }
            }
            UiEvent::Wheel { pos, delta } if self.rect.contains(pos) => {
                return self.scrollbar.scroll_by(-delta * 2);
            }
            UiEvent::Key { code, pressed: true, .. } if self.focused && !self.rows.is_empty() => {
                match code {
                    103 => {
                        // Up arrow
                        let next = self.selected.map(|s| s.saturating_sub(1)).unwrap_or(0);
                        self.selected = Some(next);
                        self.ensure_visible(next);
                        return true;
                    }
                    108 => {
                        // Down arrow
                        let next = self
                            .selected
                            .map(|s| (s + 1).min(self.rows.len() - 1))
                            .unwrap_or(0);
                        self.selected = Some(next);
                        self.ensure_visible(next);
                        return true;
                    }
                    28 => {
                        // Enter
                        if let Some(sel) = self.selected {
                            self.activated = Some(sel);
                            return true;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        false
    }
}
