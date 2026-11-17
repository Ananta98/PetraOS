//! MenuBar, Menu, and MenuItem widgets (SerenityOS `LibGUI::Menubar`/`Menu` pattern).

use crate::painter::StylePainter;
use crate::theme::Theme;
use crate::traits::{EventListener, Layoutable, Paintable, UiEvent};
use space_core::font;
use space_core::{Canvas, Color, Point, Rect};

#[derive(Debug, Clone)]
pub struct MenuItem {
    pub id: &'static str,
    pub label: String,
    pub shortcut: String,
    pub separator: bool,
}

impl MenuItem {
    pub fn action(id: &'static str, label: impl Into<String>, shortcut: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            shortcut: shortcut.into(),
            separator: false,
        }
    }

    pub fn separator() -> Self {
        Self {
            id: "",
            label: String::new(),
            shortcut: String::new(),
            separator: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Menu {
    pub title: String,
    pub items: Vec<MenuItem>,
}

impl Menu {
    pub fn new(title: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Self {
            title: title.into(),
            items,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MenuBar {
    pub rect: Rect,
    pub menus: Vec<Menu>,
    pub open_index: Option<usize>,
    pub hovered_header: Option<usize>,
    pub hovered_item: Option<usize>,
    pub triggered: Option<&'static str>,
}

impl MenuBar {
    pub fn new(rect: Rect, menus: Vec<Menu>) -> Self {
        Self {
            rect,
            menus,
            open_index: None,
            hovered_header: None,
            hovered_item: None,
            triggered: None,
        }
    }

    pub fn take_action(&mut self) -> Option<&'static str> {
        self.triggered.take()
    }

    pub fn is_open(&self) -> bool {
        self.open_index.is_some()
    }

    fn header_rect(&self, idx: usize) -> Rect {
        let mut x = self.rect.x + 4;
        for (i, m) in self.menus.iter().enumerate() {
            let w = font::measure_text_width(&m.title, 2) + 18;
            if i == idx {
                return Rect::new(x, self.rect.y + 2, w, (self.rect.h - 4).max(16));
            }
            x += w + 2;
        }
        Rect::new(0, 0, 0, 0)
    }

    fn dropdown_rect(&self, idx: usize) -> Rect {
        let hr = self.header_rect(idx);
        let Some(menu) = self.menus.get(idx) else {
            return Rect::new(0, 0, 0, 0);
        };
        let item_h = 24;
        let mut h = 8;
        let mut w = 160i32;
        for item in &menu.items {
            h += if item.separator { 8 } else { item_h };
            if !item.separator {
                let lw = font::measure_text_width(&item.label, 2);
                let sw = if item.shortcut.is_empty() { 0 } else { font::measure_text_width(&item.shortcut, 1) + 18 };
                w = w.max(lw + sw + 24);
            }
        }
        Rect::new(hr.x, self.rect.y + self.rect.h, w, h)
    }

    fn item_at(&self, menu_idx: usize, pos: Point) -> Option<usize> {
        let dr = self.dropdown_rect(menu_idx);
        if !dr.contains(pos) {
            return None;
        }
        let menu = self.menus.get(menu_idx)?;
        let mut y = dr.y + 4;
        for (i, item) in menu.items.iter().enumerate() {
            let h = if item.separator { 8 } else { 24 };
            if pos.y >= y && pos.y < y + h {
                return if item.separator { None } else { Some(i) };
            }
            y += h;
        }
        None
    }

    /// Paints the dropdown popup; call after painting main content so it sits on top.
    pub fn paint_popup(&self, canvas: &mut dyn Canvas) {
        let Some(idx) = self.open_index else {
            return;
        };
        let Some(menu) = self.menus.get(idx) else {
            return;
        };
        let t = Theme::PETRA_SLATE;
        let dr = self.dropdown_rect(idx);
        canvas.fill_rect(Rect::new(dr.x + 2, dr.y + 2, dr.w, dr.h), Color::argb(65, 0, 0, 0));
        canvas.fill_rect(dr, t.bg_input);
        canvas.draw_rect_outline(dr, 1, t.border);

        let mut y = dr.y + 4;
        for (i, item) in menu.items.iter().enumerate() {
            if item.separator {
                StylePainter::new(&t).paint_divider(canvas, Rect::new(dr.x + 6, y + 3, dr.w - 12, 2), false);
                y += 8;
                continue;
            }
            let ir = Rect::new(dr.x + 3, y, dr.w - 6, 24);
            let hovered = self.hovered_item == Some(i);
            if hovered {
                canvas.fill_rect(ir, t.accent);
            }
            let fg = if hovered { t.text_on_accent } else { t.text_primary };
            let ty = y + (24 - font::GLYPH_H * 2) / 2;
            font::draw_text(canvas, ir.x + 8, ty, &item.label, fg, 2);
            if !item.shortcut.is_empty() {
                let sw = font::measure_text_width(&item.shortcut, 1);
                let sc_col = if hovered { t.text_on_accent } else { t.text_secondary };
                font::draw_text(canvas, ir.x + ir.w - sw - 8, ty + 2, &item.shortcut, sc_col, 1);
            }
            y += 24;
        }
    }
}

impl Layoutable for MenuBar {
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
}

impl Paintable for MenuBar {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::PETRA_SLATE;
        StylePainter::new(&t).paint_raised_bar(canvas, self.rect);
        for (i, menu) in self.menus.iter().enumerate() {
            let hr = self.header_rect(i);
            if hr.right() > self.rect.right() {
                break;
            }
            let active = self.open_index == Some(i) || self.hovered_header == Some(i);
            if active {
                canvas.fill_rect(hr, t.accent);
            }
            let fg = if active { t.text_on_accent } else { t.text_primary };
            let ty = hr.y + (hr.h - font::GLYPH_H * 2) / 2;
            font::draw_text(canvas, hr.x + 9, ty, &menu.title, fg, 2);
        }
    }
}

impl EventListener for MenuBar {
    fn on_event(&mut self, event: UiEvent) -> bool {
        match event {
            UiEvent::MouseMove { pos, .. } => {
                let mut changed = false;
                let prev_h = self.hovered_header;
                self.hovered_header = (0..self.menus.len()).find(|&i| self.header_rect(i).contains(pos));
                if prev_h != self.hovered_header {
                    changed = true;
                    if self.open_index.is_some() {
                        if let Some(h) = self.hovered_header {
                            self.open_index = Some(h);
                        }
                    }
                }
                if let Some(open) = self.open_index {
                    let prev_i = self.hovered_item;
                    self.hovered_item = self.item_at(open, pos);
                    if prev_i != self.hovered_item {
                        changed = true;
                    }
                }
                return changed;
            }
            UiEvent::MouseDown { pos, button: 1 } => {
                if let Some(idx) = (0..self.menus.len()).find(|&i| self.header_rect(i).contains(pos)) {
                    self.open_index = if self.open_index == Some(idx) { None } else { Some(idx) };
                    self.hovered_item = None;
                    return true;
                }
                if let Some(open) = self.open_index {
                    if let Some(item_idx) = self.item_at(open, pos) {
                        if let Some(item) = self.menus.get(open).and_then(|m| m.items.get(item_idx)) {
                            self.triggered = Some(item.id);
                        }
                    }
                    self.open_index = None;
                    self.hovered_item = None;
                    return true;
                }
            }
            UiEvent::Key { code: 1, pressed: true, .. } if self.open_index.is_some() => {
                self.open_index = None;
                self.hovered_item = None;
                return true;
            }
            _ => {}
        }
        false
    }
}
